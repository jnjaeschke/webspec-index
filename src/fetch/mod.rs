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

/// Fetch and index a spec at a specific SHA (or latest if None)
/// Returns the snapshot ID
pub async fn fetch_and_index(
    conn: &Connection,
    spec: &SpecInfo,
    sha: Option<&str>,
    provider: &(dyn SpecProvider + Send + Sync),
) -> Result<i64> {
    // Determine SHA to fetch
    let (target_sha, commit_date) = if let Some(sha) = sha {
        // Use provided SHA and fetch its date
        let date = provider.fetch_version_date(spec, sha).await?;
        (sha.to_string(), date.to_rfc3339())
    } else {
        // Fetch latest
        let (sha, date) = provider.fetch_latest_version(spec).await?;
        (sha, date.to_rfc3339())
    };

    // Check if this snapshot already exists
    if let Some(existing_id) = queries::get_snapshot_by_sha(conn, &spec.name, &target_sha)? {
        return Ok(existing_id);
    }

    // Fetch HTML
    let html = provider.fetch_html(spec, &target_sha).await?;

    // Parse the spec
    let parsed = parse::parse_spec(&html, spec.name, spec.base_url)?;

    // Insert into database
    let spec_id = write::insert_or_get_spec(conn, &spec.name, &spec.base_url, &spec.provider)?;
    let snapshot_id = write::insert_snapshot(conn, spec_id, &target_sha, &commit_date)?;
    write::insert_sections_bulk(conn, snapshot_id, &parsed.sections)?;
    write::insert_refs_bulk(conn, snapshot_id, &parsed.references)?;

    // Set as latest if we fetched the latest version
    if sha.is_none() {
        write::set_latest_snapshot(conn, spec_id, snapshot_id)?;
    }

    // Record update check
    write::record_update_check(conn, spec_id)?;

    Ok(snapshot_id)
}

/// Ensure the latest version of a spec is indexed
/// Returns the snapshot ID of the latest version
pub async fn ensure_latest_indexed(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
) -> Result<i64> {
    // Check if we already have a latest snapshot
    if let Some(snapshot_id) = queries::get_latest_snapshot(conn, &spec.name)? {
        return Ok(snapshot_id);
    }

    // If not, fetch and index the latest
    fetch_and_index(conn, spec, None, provider).await
}

/// Update a spec to the latest version if needed
/// Returns Some(snapshot_id) if updated, None if already up to date
pub async fn update_if_needed(
    conn: &Connection,
    spec: &SpecInfo,
    provider: &(dyn SpecProvider + Send + Sync),
    force: bool,
) -> Result<Option<i64>> {
    let spec_id = write::insert_or_get_spec(conn, &spec.name, &spec.base_url, &spec.provider)?;

    // Check if we need to update (24h throttle unless forced)
    if !force {
        // Check last update time
        let last_checked: Option<String> = conn
            .query_row(
                "SELECT last_checked FROM update_checks WHERE spec_id = ?1",
                [spec_id],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(last_checked_str) = &last_checked {
            if let Ok(last_checked) =
                chrono::DateTime::parse_from_rfc3339(&last_checked_str)
            {
                let now = chrono::Utc::now();
                let duration = now.signed_duration_since(last_checked);
                if duration.num_hours() < 24 {
                    // Too soon, skip update
                    return Ok(None);
                }
            }
        }
    }

    // Get latest version from provider
    let (latest_sha, _) = provider.fetch_latest_version(spec).await?;

    // Check if we already have this SHA
    if queries::get_snapshot_by_sha(conn, &spec.name, &latest_sha)?.is_some() {
        // We already have the latest version
        write::record_update_check(conn, spec_id)?;
        return Ok(None);
    }

    // Fetch and index the new version
    let snapshot_id = fetch_and_index(conn, spec, Some(&latest_sha), provider).await?;
    write::set_latest_snapshot(conn, spec_id, snapshot_id)?;

    Ok(Some(snapshot_id))
}

/// Update all specs in the registry
/// Returns vector of (spec_name, Option<snapshot_id>) pairs
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
