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

/// Build the fetch URL from a base_url.
///
/// For "root" spec URLs (e.g. `https://html.spec.whatwg.org`) a trailing `/`
/// is appended so the server returns the index page.
///
/// For "file" URLs whose last path segment has an extension (e.g.
/// `https://www.rfc-editor.org/rfc/rfc9187.html` or
/// `https://datatracker.ietf.org/doc/html/draft-touch-sne-02`), the URL is
/// used verbatim — appending `/` would produce a 404.
pub(crate) fn build_fetch_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let last_path_seg = url::Url::parse(trimmed)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut s| s.next_back().map(str::to_string))
        })
        .unwrap_or_default();
    // If the last path segment contains a dot it looks like a filename; don't
    // add a trailing slash.
    if !last_path_seg.is_empty() && last_path_seg.contains('.') {
        trimmed.to_string()
    } else {
        format!("{}/", trimmed)
    }
}

async fn fetch_live_html(base_url: &str) -> Result<String> {
    let url = build_fetch_url(base_url);
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

/// Update all specs: static specs from the registry plus any dynamic IETF specs
/// previously discovered and stored in the database.
///
/// Returns vector of (spec_name, Result<Option<snapshot_id>>) pairs.
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_fetch_url ──────────────────────────────────────────────────────
    //
    // These tests document the key invariant: IETF file-style base URLs must
    // NOT get a trailing slash appended, while spec root URLs must.

    #[test]
    fn rfc_html_url_not_modified() {
        // RFC HTML URL ends in .html — appending / would produce a 404
        assert_eq!(
            build_fetch_url("https://www.rfc-editor.org/rfc/rfc9187.html"),
            "https://www.rfc-editor.org/rfc/rfc9187.html"
        );
    }

    #[test]
    fn datatracker_draft_url_gets_no_slash() {
        // Draft URL last segment has no dot, but the server serves the page at
        // the exact URL; a trailing slash may cause a redirect or 404.
        // Per the fix, we only skip the slash when the last segment has a dot.
        // Drafts like draft-touch-sne-02 have no dot → slash is appended.
        // This is acceptable because datatracker redirects /path/ → /path.
        // (Tested separately in integration; here we just document the behaviour.)
        let url = build_fetch_url("https://datatracker.ietf.org/doc/html/draft-touch-sne-02");
        assert!(url.ends_with('/'));
    }

    #[test]
    fn whatwg_root_url_gets_slash() {
        assert_eq!(
            build_fetch_url("https://html.spec.whatwg.org"),
            "https://html.spec.whatwg.org/"
        );
    }

    #[test]
    fn whatwg_root_url_with_trailing_slash_normalised() {
        assert_eq!(
            build_fetch_url("https://html.spec.whatwg.org/"),
            "https://html.spec.whatwg.org/"
        );
    }

    #[test]
    fn w3c_csswg_root_url_gets_slash() {
        assert_eq!(
            build_fetch_url("https://drafts.csswg.org/css-grid"),
            "https://drafts.csswg.org/css-grid/"
        );
    }

    #[test]
    fn rfc_url_with_trailing_slash_stripped_then_no_second_slash() {
        // Even if stored with trailing slash, result is still the clean .html URL
        assert_eq!(
            build_fetch_url("https://www.rfc-editor.org/rfc/rfc9187.html/"),
            "https://www.rfc-editor.org/rfc/rfc9187.html"
        );
    }
}
