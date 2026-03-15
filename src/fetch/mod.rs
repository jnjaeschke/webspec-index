// Fetch orchestration: coordinate HTML fetching, parsing, and database writes
pub mod github;
pub mod snapshot;

use crate::db::{queries, write};
use crate::parse;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

const CHECK_INTERVAL_HOURS: i64 = 24;

fn is_fresh(last_checked: &DateTime<Utc>, now: &DateTime<Utc>) -> bool {
    now.signed_duration_since(*last_checked).num_hours() < CHECK_INTERVAL_HOURS
}

fn hash_html(html: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(html.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn store_update_check(
    conn: &Connection,
    spec_id: i64,
    now: &DateTime<Utc>,
    last_indexed: Option<&DateTime<Utc>>,
    content_hash: Option<&str>,
) -> Result<()> {
    let checked = now.to_rfc3339();
    let indexed = last_indexed.map(|t| t.to_rfc3339());
    write::record_update_check(conn, spec_id, &checked, indexed.as_deref(), content_hash)
}

#[allow(clippy::too_many_arguments)]
fn sync_from_html(
    conn: &Connection,
    spec_id: i64,
    spec_name: &str,
    base_url: &str,
    provider_name: &str,
    html: String,
    previous_snapshot_id: Option<i64>,
    state: Option<queries::UpdateCheckState>,
    now: &DateTime<Utc>,
) -> Result<(i64, bool)> {
    let content_hash = hash_html(&html);

    if let Some(snapshot_id) = previous_snapshot_id {
        if state.as_ref().and_then(|s| s.content_hash.as_deref()) == Some(content_hash.as_str()) {
            let existing_indexed = state.as_ref().and_then(|s| s.last_indexed.as_ref());
            store_update_check(conn, spec_id, now, existing_indexed, Some(&content_hash))?;
            return Ok((snapshot_id, false));
        }
    }

    let parsed = parse::parse_spec(&html, spec_name, base_url)?;
    write::delete_spec_data(conn, spec_id)?;

    let synthetic_sha = format!("hash:{content_hash}");
    let commit_date = now.to_rfc3339();
    let spec_id_reloaded = write::insert_or_get_spec(conn, spec_name, base_url, provider_name)?;
    let snapshot_id = write::insert_snapshot(conn, spec_id_reloaded, &synthetic_sha, &commit_date)?;
    write::insert_sections_bulk(conn, snapshot_id, &parsed.sections)?;
    write::insert_refs_bulk(conn, snapshot_id, &parsed.references)?;
    write::insert_idl_defs_bulk(conn, snapshot_id, &parsed.idl_definitions)?;

    store_update_check(conn, spec_id_reloaded, now, Some(now), Some(&content_hash))?;
    Ok((snapshot_id, true))
}

fn is_respec_source(html: &str) -> bool {
    html.contains("respec-w3c") || html.contains("respec.js") || html.contains("/respec/")
}

async fn render_via_spec_generator(url: &str) -> Result<String> {
    let api_url = format!(
        "https://www.w3.org/publications/spec-generator/?type=respec&url={}",
        url::form_urlencoded::byte_serialize(url.as_bytes()).collect::<String>()
    );
    let html = fetch_raw_html(&api_url).await?;
    if html.trim_start().starts_with('{') {
        anyhow::bail!(
            "spec-generator returned error: {}",
            &html[..html.len().min(200)]
        );
    }
    Ok(html)
}

async fn fetch_raw_html(url: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", "webspec-index/0.5.0")
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("Failed to fetch {}: HTTP {}", url, response.status());
    }
    Ok(response.text().await?)
}

async fn fetch_live_html(base_url: &str) -> Result<String> {
    let url = if base_url.ends_with(".html") || base_url.ends_with(".txt") {
        base_url.to_string()
    } else {
        format!("{}/", base_url.trim_end_matches('/'))
    };
    let html = fetch_raw_html(&url).await?;

    if is_respec_source(&html) {
        eprintln!(
            "note: {} is a live ReSpec document; rendering via W3C spec-generator",
            url
        );
        match render_via_spec_generator(&url).await {
            Ok(rendered) => return Ok(rendered),
            Err(e) => eprintln!("warning: spec-generator failed ({}), using raw HTML", e),
        }
    }

    Ok(html)
}

async fn sync_known_spec(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
    provider_name: &str,
    force: bool,
) -> Result<(i64, bool)> {
    let spec_id = write::insert_or_get_spec(conn, spec_name, base_url, provider_name)?;
    let previous_snapshot_id = queries::get_snapshot(conn, spec_name)?;
    let state = queries::get_update_check(conn, spec_id)?;
    let now = Utc::now();

    if !force {
        if let (Some(snapshot_id), Some(sync_state)) = (previous_snapshot_id, state.as_ref()) {
            if is_fresh(&sync_state.last_checked, &now) {
                return Ok((snapshot_id, false));
            }
        }
    }

    let html = fetch_live_html(base_url).await?;
    sync_from_html(
        conn,
        spec_id,
        spec_name,
        base_url,
        provider_name,
        html,
        previous_snapshot_id,
        state,
        &now,
    )
}

async fn sync_dynamic_spec(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
    force: bool,
) -> Result<(i64, bool)> {
    let spec_id = write::insert_or_get_spec(conn, spec_name, base_url, "dynamic")?;
    let previous_snapshot_id = queries::get_snapshot(conn, spec_name)?;
    let state = queries::get_update_check(conn, spec_id)?;
    let now = Utc::now();

    if !force {
        if let (Some(snapshot_id), Some(sync_state)) = (previous_snapshot_id, state.as_ref()) {
            if is_fresh(&sync_state.last_checked, &now) {
                return Ok((snapshot_id, false));
            }
        }
    }

    let html = fetch_live_html(base_url).await?;
    sync_from_html(
        conn,
        spec_id,
        spec_name,
        base_url,
        "dynamic",
        html,
        previous_snapshot_id,
        state,
        &now,
    )
}

/// Ensure an ad-hoc URL-based spec is indexed.
///
/// This supports domains accepted by `SpecRegistry::resolve_url()` auto resolution.
pub async fn ensure_indexed_dynamic(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
) -> Result<i64> {
    let (snapshot_id, _) = sync_dynamic_spec(conn, spec_name, base_url, false).await?;
    Ok(snapshot_id)
}

/// Ensure a spec is indexed and reasonably fresh.
///
/// Uses a 24h freshness window based on `update_checks.last_checked`.
/// When refreshing, fetches live HTML and re-indexes only if content hash changed.
pub async fn ensure_indexed(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
    provider_name: &str,
) -> Result<i64> {
    let (snapshot_id, _) = sync_known_spec(conn, spec_name, base_url, provider_name, false).await?;
    Ok(snapshot_id)
}

/// Update a spec if needed.
///
/// Returns `Some(snapshot_id)` only when content changed and was re-indexed.
pub async fn update_if_needed(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
    provider_name: &str,
    force: bool,
) -> Result<Option<i64>> {
    let (snapshot_id, updated) =
        sync_known_spec(conn, spec_name, base_url, provider_name, force).await?;
    Ok(updated.then_some(snapshot_id))
}

/// Update all specs in the registry.
/// Returns vector of (spec_name, Option<snapshot_id>) pairs.
pub async fn update_all_specs(
    conn: &Connection,
    specs: &[(String, String, String)], // (name, base_url, provider)
    force: bool,
) -> Vec<(String, Result<Option<i64>>)> {
    let mut results = Vec::new();

    for (name, base_url, provider) in specs {
        let result = update_if_needed(conn, name, base_url, provider, force).await;
        results.push((name.clone(), result));
    }

    results
}
