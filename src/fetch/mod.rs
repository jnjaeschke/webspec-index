// Fetch orchestration: coordinate HTML fetching, parsing, and database writes
pub mod github;
pub mod snapshot;

use crate::db::{queries, write};
use crate::parse;
use crate::provider::SpecAccess;
use crate::spec_registry::SpecRegistry;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

/// Get the latest SHA for a spec's GitHub repo, using the DB cache (24h TTL).
/// Only calls the GitHub API when the cache is missing or stale (or force=true).
/// The cache key is the repo path (e.g. "w3c/csswg-drafts"), so all specs
/// sharing a monorepo make at most one API call per 24h window.
async fn get_latest_sha(
    conn: &Connection,
    spec: &dyn SpecAccess,
    force: bool,
) -> Result<(String, DateTime<Utc>)> {
    if !force {
        if let Some((sha, commit_date, checked_at)) =
            queries::get_repo_sha_cache(conn, spec.version_cache_key())?
        {
            let age = Utc::now().signed_duration_since(checked_at);
            if age.num_hours() < 24 {
                return Ok((sha, commit_date));
            }
        }
    }

    // Cache miss or stale: call GitHub API and refresh the cache
    let (sha, date) = spec.fetch_latest_version().await?;
    write::update_repo_sha_cache(conn, spec.version_cache_key(), &sha, &date)?;
    Ok((sha, date))
}

/// Fetch the spec HTML, parse it, and write to the database.
/// The SHA and date are provided by the caller (already fetched/cached).
async fn do_index(
    conn: &Connection,
    spec: &dyn SpecAccess,
    sha: &str,
    date: &DateTime<Utc>,
) -> Result<i64> {
    let html = spec.fetch_html(sha).await?;
    let parsed = parse::parse_spec(&html, spec.name(), spec.url())?;

    let spec_id = write::insert_or_get_spec(conn, spec.name(), spec.url(), spec.provider())?;
    write::delete_spec_data(conn, spec_id)?;

    let snapshot_id = write::insert_snapshot(conn, spec_id, sha, &date.to_rfc3339())?;
    write::insert_sections_bulk(conn, snapshot_id, &parsed.sections)?;
    write::insert_refs_bulk(conn, snapshot_id, &parsed.references)?;

    Ok(snapshot_id)
}

/// Ensure a spec is indexed and up to date.
///
/// On every call, checks the repo-level SHA cache (24h TTL) and re-indexes
/// the spec if the upstream SHA has advanced. This is the lazy-update entry
/// point used by all query paths.
pub async fn ensure_indexed(conn: &Connection, spec: &dyn SpecAccess) -> Result<i64> {
    let (sha, date) = get_latest_sha(conn, spec, false).await?;

    if let Some(snapshot_id) = queries::get_snapshot_by_sha(conn, spec.name(), &sha)? {
        return Ok(snapshot_id);
    }

    do_index(conn, spec, &sha, &date).await
}

/// Update a spec if a newer version is available.
///
/// Returns `Some(snapshot_id)` if the spec was re-indexed, `None` if already
/// at the latest SHA. Respects the 24h repo cache unless `force` is true.
pub async fn update_if_needed(
    conn: &Connection,
    spec: &dyn SpecAccess,
    force: bool,
) -> Result<Option<i64>> {
    let (sha, date) = get_latest_sha(conn, spec, force).await?;

    if queries::get_snapshot_by_sha(conn, spec.name(), &sha)?.is_some() {
        return Ok(None);
    }

    Ok(Some(do_index(conn, spec, &sha, &date).await?))
}

/// Update all specs in the registry.
/// Returns vector of (spec_name, Option<snapshot_id>) pairs.
pub async fn update_all_specs(
    conn: &Connection,
    registry: &SpecRegistry,
    force: bool,
) -> Vec<(String, Result<Option<i64>>)> {
    let mut results = Vec::new();

    for spec in registry.list_all_specs() {
        let result = update_if_needed(conn, spec.as_ref(), force).await;
        results.push((spec.name().to_string(), result));
    }

    results
}
