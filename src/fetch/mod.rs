// Fetch orchestration: coordinate HTML fetching, parsing, and database writes
pub mod github;
pub mod snapshot;

use crate::db::{queries, write};
use crate::model::SpecInfo;
use crate::parse;
use crate::provider::SpecProvider;
use crate::spec_registry::SpecRegistry;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

/// Fetch the latest version of a spec, parse it, and store in the database.
/// Enforces exactly one snapshot per spec: deletes old data before inserting new.
/// Returns the snapshot ID. Skips work if the SHA hasn't changed (dedup).
pub async fn fetch_and_index(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
) -> Result<i64> {
    // Get latest version from provider
    let (target_sha, commit_date) = provider.fetch_latest_version(spec).await?;
    let commit_date = commit_date.to_rfc3339();

    // Check if we already have this SHA (dedup: skip if unchanged)
    if let Some(existing_id) = queries::get_snapshot_by_sha(conn, spec.name, &target_sha)? {
        return Ok(existing_id);
    }

    // Fetch HTML
    let html = provider.fetch_html(spec, &target_sha).await?;

    // Parse the spec
    let parsed = parse::parse_spec(&html, spec.name, spec.base_url)?;

    // Delete old snapshot data for this spec (enforce 1 snapshot per spec)
    let spec_id = write::insert_or_get_spec(conn, spec.name, spec.base_url, spec.provider)?;
    write::delete_spec_data(conn, spec_id)?;

    // Insert new snapshot + sections + refs
    let snapshot_id = write::insert_snapshot(conn, spec_id, &target_sha, &commit_date)?;
    write::insert_sections_bulk(conn, snapshot_id, &parsed.sections)?;
    write::insert_refs_bulk(conn, snapshot_id, &parsed.references)?;

    // Record update check
    write::record_update_check(conn, spec_id)?;

    Ok(snapshot_id)
}

/// Ensure a spec is indexed. Returns the snapshot ID.
/// If already indexed, returns existing. Otherwise fetches and indexes.
pub async fn ensure_indexed(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
) -> Result<i64> {
    // Check if we already have a snapshot for this spec
    if let Some(snapshot_id) = queries::get_snapshot(conn, spec.name)? {
        return Ok(snapshot_id);
    }

    // If not, fetch and index
    fetch_and_index(conn, spec, provider).await
}

/// Update a spec to the latest version if needed.
/// Returns Some(snapshot_id) if updated, None if already up to date.
pub async fn update_if_needed(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
    force: bool,
) -> Result<Option<i64>> {
    let spec_id = write::insert_or_get_spec(conn, spec.name, spec.base_url, spec.provider)?;

    // Check if we need to update (24h throttle unless forced)
    if !force {
        let last_checked: Option<String> = conn
            .query_row(
                "SELECT last_checked FROM update_checks WHERE spec_id = ?1",
                [spec_id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(last_checked_str) = &last_checked {
            if let Ok(last_checked) = chrono::DateTime::parse_from_rfc3339(last_checked_str) {
                let now = chrono::Utc::now();
                let duration = now.signed_duration_since(last_checked);
                if duration.num_hours() < 24 {
                    return Ok(None);
                }
            }
        }
    }

    // Get latest version from provider
    let (latest_sha, _) = provider.fetch_latest_version(spec).await?;

    // Check if we already have this SHA (no change needed)
    if queries::get_snapshot_by_sha(conn, spec.name, &latest_sha)?.is_some() {
        write::record_update_check(conn, spec_id)?;
        return Ok(None);
    }

    // Fetch and index the new version (deletes old data internally)
    let snapshot_id = fetch_and_index(conn, spec, provider).await?;

    Ok(Some(snapshot_id))
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
