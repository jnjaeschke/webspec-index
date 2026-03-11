// Fetch orchestration: coordinate HTML fetching, parsing, and database writes
pub mod github;
pub mod snapshot;

use crate::db::{queries, write};
use crate::model::SpecInfo;
use crate::parse;
use crate::provider::SpecProvider;
use crate::spec_registry::SpecRegistry;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

/// Cache key for repo-level SHA tracking.
/// For providers that share a GitHub monorepo (e.g. W3C CSSWG), the repo path
/// is used so a single API call serves all specs in the repo.
/// For providers without a GitHub repo (e.g. IETF), the spec name is used as
/// a per-document key so each doc gets its own 24-hour cache entry.
fn sha_cache_key(spec: &SpecInfo) -> &str {
    if spec.github_repo.is_empty() {
        spec.name
    } else {
        spec.github_repo
    }
}

/// Get the latest SHA for a spec, using the DB cache (24h TTL).
/// Only calls the upstream API when the cache is missing or stale (or force=true).
async fn get_latest_sha(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
    force: bool,
) -> Result<(String, DateTime<Utc>)> {
    let cache_key = sha_cache_key(spec);

    if !force {
        if let Some((sha, commit_date, checked_at)) =
            queries::get_repo_sha_cache(conn, cache_key)?
        {
            let age = Utc::now().signed_duration_since(checked_at);
            if age.num_hours() < 24 {
                return Ok((sha, commit_date));
            }
        }
    }

    // Cache miss or stale: call the provider's upstream API and refresh the cache
    let (sha, date) = provider.fetch_latest_version(spec).await?;
    write::update_repo_sha_cache(conn, cache_key, &sha, &date)?;
    Ok((sha, date))
}

/// Fetch the spec HTML, parse it, and write to the database.
/// The SHA and date are provided by the caller (already fetched/cached).
async fn do_index(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
    sha: &str,
    date: &DateTime<Utc>,
) -> Result<i64> {
    let html = provider.fetch_html(spec, sha).await?;
    let parsed = parse::parse_spec(&html, spec.name, spec.base_url)?;

    let spec_id = write::insert_or_get_spec(conn, spec.name, spec.base_url, spec.provider)?;
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
pub async fn ensure_indexed(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
) -> Result<i64> {
    let (sha, date) = get_latest_sha(conn, spec, provider, false).await?;

    if let Some(snapshot_id) = queries::get_snapshot_by_sha(conn, spec.name, &sha)? {
        return Ok(snapshot_id);
    }

    do_index(conn, spec, provider, &sha, &date).await
}

/// Update a spec if a newer version is available.
///
/// Returns `Some(snapshot_id)` if the spec was re-indexed, `None` if already
/// at the latest SHA. Respects the 24h repo cache unless `force` is true.
pub async fn update_if_needed(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
    force: bool,
) -> Result<Option<i64>> {
    let (sha, date) = get_latest_sha(conn, spec, provider, force).await?;

    if queries::get_snapshot_by_sha(conn, spec.name, &sha)?.is_some() {
        return Ok(None);
    }

    Ok(Some(do_index(conn, spec, provider, &sha, &date).await?))
}

/// Update all specs: static specs from the registry plus any dynamic IETF specs
/// previously discovered and stored in the database.
///
/// Returns vector of (spec_name, Result<Option<snapshot_id>>) pairs.
pub async fn update_all_specs(
    conn: &Connection,
    registry: &SpecRegistry,
    force: bool,
) -> Vec<(String, Result<Option<i64>>)> {
    let mut results = Vec::new();

    // Static specs (WHATWG, W3C, TC39)
    for spec in registry.list_all_specs() {
        let provider = match registry.get_provider(spec) {
            Ok(p) => p,
            Err(e) => {
                results.push((spec.name.to_string(), Err(e)));
                continue;
            }
        };
        let result = update_if_needed(conn, spec, provider, force).await;
        results.push((spec.name.to_string(), result));
    }

    // Dynamic IETF specs previously discovered and stored in the DB
    let ietf_specs = queries::list_specs_by_provider(conn, "ietf").unwrap_or_default();
    for spec in &ietf_specs {
        let provider = match registry.get_provider(spec) {
            Ok(p) => p,
            Err(e) => {
                results.push((spec.name.to_string(), Err(e)));
                continue;
            }
        };
        let result = update_if_needed(conn, spec, provider, force).await;
        results.push((spec.name.to_string(), result));
    }

    results
}
