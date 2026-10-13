// Provider-agnostic PR-preview orchestration.
//
// Each provider implements `PrResolver` to turn a PR into a `ResolvedPr` of
// concrete fetch URLs (WHATWG via whatpr.org, TC39 proposals via the GitHub
// API). Everything below — caching, merge-base fetch/reuse, page fetching,
// snapshot storage — is shared and only ever fetches raw HTML by URL.

use crate::db::{queries, write};
use crate::model::ParsedSpec;
use crate::parse;
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::Connection;

/// A single preview page to fetch and index.
#[derive(Debug, Clone)]
pub struct PrPage {
    pub page_path: String,
    pub url: String,
    pub diff_url: Option<String>,
}

/// A PR preview resolved to concrete fetch URLs, independent of provider.
pub(crate) struct ResolvedPr {
    /// Native head identifier stored in the PR snapshot key (short SHA for
    /// whatpr.org, full SHA for TC39). Compared with `ends_with` against the
    /// stored key, so either width works.
    pub head_sha: String,
    /// Full merge-base SHA, used as the commit-snapshot cache key.
    pub merge_base_sha: String,
    /// URL of the merge-base build to diff against.
    pub base_html_url: String,
    /// PR build page(s) to fetch and merge.
    pub pages: Vec<PrPage>,
}

/// Resolve a PR to concrete fetch URLs. Implemented per provider so the shared
/// orchestration in `ensure_pr_indexed` stays provider-agnostic.
// async_trait adds `#[must_use]` to methods that already return a `Future`.
#[allow(clippy::double_must_use)]
#[async_trait]
pub(crate) trait PrResolver {
    async fn resolve(&self, spec_name: &str, base_url: &str, pr_number: i64) -> Result<ResolvedPr>;
}

/// Pick the resolver for a provider.
fn resolver_for(provider: &str) -> Result<Box<dyn PrResolver>> {
    match provider {
        "whatwg" => Ok(Box::new(super::whatpr::WhatwgResolver)),
        "tc39" => Ok(Box::new(super::tc39_pr::Tc39Resolver)),
        other => anyhow::bail!(
            "PR previews are not supported for provider '{other}' — only WHATWG specs and TC39 proposals"
        ),
    }
}

/// Merge multiple ParsedSpec results (from multi-page fetches) into one.
///
/// Multi-page previews can repeat an anchor across pages (a shared section
/// appearing on two pages). Sections, references, and IDL definitions are all
/// deduplicated by identity so the extras don't accumulate — the storage layer
/// does not dedup, so a duplicate would otherwise surface as repeated `refs`/
/// `idl` entries in the merged preview.
pub fn merge_parsed_specs(specs: Vec<ParsedSpec>) -> ParsedSpec {
    let mut seen_anchors = std::collections::HashSet::new();
    let mut seen_refs = std::collections::HashSet::new();
    let mut seen_idl = std::collections::HashSet::new();
    let mut sections = Vec::new();
    let mut references = Vec::new();
    let mut idl_definitions = Vec::new();
    for spec in specs {
        for section in spec.sections {
            if seen_anchors.insert(section.anchor.clone()) {
                sections.push(section);
            }
        }
        for r in spec.references {
            if seen_refs.insert((
                r.from_anchor.clone(),
                r.to_spec.clone(),
                r.to_anchor.clone(),
            )) {
                references.push(r);
            }
        }
        for d in spec.idl_definitions {
            if seen_idl.insert((d.anchor.clone(), d.canonical_name.clone())) {
                idl_definitions.push(d);
            }
        }
    }
    ParsedSpec {
        sections,
        references,
        idl_definitions,
    }
}

/// Fetch and parse every preview page, merging them into one spec.
async fn fetch_pr_pages(
    pages: &[PrPage],
    spec_name: &str,
    base_url: &str,
    pr_number: i64,
) -> Result<ParsedSpec> {
    let mut parsed_pages = Vec::new();
    for page in pages {
        eprintln!("Fetching PR #{pr_number} page: {}", page.page_path);
        let html = super::fetch_raw_html(&page.url).await?;
        let parsed = parse::parse_spec(&html, spec_name, base_url)?;
        parsed_pages.push(parsed);
    }
    Ok(merge_parsed_specs(parsed_pages))
}

/// Whether a snapshot was produced by the current build. An `INDEX_VERSION`
/// bump makes cached snapshots (including reused merge bases) stale, so they
/// must be re-fetched and re-parsed. A legacy integer value written by an
/// earlier build fails the text read and is treated as stale.
fn snapshot_index_is_current(conn: &Connection, snapshot_id: i64) -> bool {
    conn.query_row(
        "SELECT index_version FROM snapshots WHERE id = ?1",
        [snapshot_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .map(|v| v.as_deref() == Some(parse::INDEX_VERSION))
    .unwrap_or(false)
}

fn is_pr_snapshot_valid(conn: &Connection, snapshot_id: i64) -> bool {
    if !snapshot_index_is_current(conn, snapshot_id) {
        return false;
    }
    conn.query_row(
        "SELECT COUNT(*) FROM sections WHERE snapshot_id = ?1",
        [snapshot_id],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

/// Truncate to at most `max_chars` characters for logging. Iterates by `char`
/// rather than slicing at a byte index, which would panic if the cut fell inside
/// a multi-byte UTF-8 sequence.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Look up a previously-cached PR snapshot and its merge-base snapshot, returning
/// their ids only when both are present. Used as the offline fallback when PR
/// resolution fails: unlike the freshness fast path, this also accepts a snapshot
/// produced by an older build rather than erroring.
fn cached_pr_fallback(
    conn: &Connection,
    spec_id: i64,
    spec_name: &str,
    pr_number: i64,
) -> Result<Option<(i64, i64)>> {
    if let Some((pr_snap_id, stored_base_sha)) =
        queries::get_pr_snapshot(conn, spec_name, pr_number)?
    {
        if let Some(base_snap_id) = queries::get_commit_snapshot(conn, spec_id, &stored_base_sha)? {
            return Ok(Some((pr_snap_id, base_snap_id)));
        }
    }
    Ok(None)
}

/// Ensure a PR snapshot is indexed and fresh.
///
/// Returns (pr_snapshot_id, merge_base_snapshot_id). Resolves the PR through the
/// matching provider's `PrResolver`, then applies shared caching and storage.
///
/// If the PR is already indexed with the same head SHA (and was indexed within
/// the last 24h when `force` is false), returns cached IDs without hitting the
/// network.
pub async fn ensure_pr_indexed(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
    provider: &str,
    pr_number: i64,
    force: bool,
) -> Result<(i64, i64)> {
    let spec_id = write::insert_or_get_spec(conn, spec_name, base_url, provider)?;

    // Fast path: if not forcing, check 24h freshness before hitting the network.
    if !force {
        if let Some((pr_snap_id, stored_base_sha)) =
            queries::get_pr_snapshot(conn, spec_name, pr_number)?
        {
            if is_pr_snapshot_valid(conn, pr_snap_id) {
                let indexed_at: String = conn.query_row(
                    "SELECT indexed_at FROM snapshots WHERE id = ?1",
                    [pr_snap_id],
                    |row| row.get(0),
                )?;
                if let Ok(indexed) = chrono::DateTime::parse_from_rfc3339(&indexed_at) {
                    let indexed_utc = indexed.with_timezone(&chrono::Utc);
                    if super::is_fresh(&indexed_utc, &chrono::Utc::now()) {
                        if let Some(base_snap_id) =
                            queries::get_commit_snapshot(conn, spec_id, &stored_base_sha)?
                        {
                            return Ok((pr_snap_id, base_snap_id));
                        }
                    }
                }
            }
        }
    }

    // Resolve the PR to concrete fetch URLs via its provider. If resolution
    // fails (e.g. offline) and a previously-cached snapshot for this PR exists
    // — even one produced by an older build, which the fast path above skips —
    // serve it as a best-effort fallback rather than erroring, mirroring the
    // trunk sync's ServeCached behavior. A forced refresh never falls back.
    let resolved = match resolver_for(provider)?
        .resolve(spec_name, base_url, pr_number)
        .await
    {
        Ok(resolved) => resolved,
        Err(e) => {
            if !force {
                if let Some((pr_snap_id, base_snap_id)) =
                    cached_pr_fallback(conn, spec_id, spec_name, pr_number)?
                {
                    eprintln!(
                        "warning: could not resolve PR #{pr_number} ({e}); \
                         serving cached preview"
                    );
                    return Ok((pr_snap_id, base_snap_id));
                }
            }
            return Err(e);
        }
    };

    // Check if we already have this PR indexed with the same head SHA.
    if let Some((pr_snap_id, stored_base_sha)) =
        queries::get_pr_snapshot(conn, spec_name, pr_number)?
    {
        let pr_sha: String = conn.query_row(
            "SELECT sha FROM snapshots WHERE id = ?1",
            [pr_snap_id],
            |row| row.get(0),
        )?;
        if pr_sha.ends_with(&resolved.head_sha) && is_pr_snapshot_valid(conn, pr_snap_id) {
            // Still current — find the merge base snapshot.
            if let Some(base_snap_id) =
                queries::get_commit_snapshot(conn, spec_id, &stored_base_sha)?
            {
                return Ok((pr_snap_id, base_snap_id));
            }
        }
        // Stale — delete old PR data.
        write::delete_pr_data(conn, spec_id, pr_number)?;
    }

    // Fetch or reuse the merge base snapshot. Reuse it only if the current
    // parser produced it AND it is non-empty; otherwise re-fetch so it stays
    // consistent with the PR snapshot (an empty base would otherwise be cached
    // forever and make every PR section look newly-added in `--diff`). The
    // re-fetch happens BEFORE deleting the stale copy so that a fetch failure
    // leaves the existing (usable) base intact rather than destroying it;
    // deleting first also avoids the UNIQUE(spec_id, sha) conflict a plain
    // re-insert would hit.
    let existing_base = queries::get_commit_snapshot(conn, spec_id, &resolved.merge_base_sha)?;
    let base_snap_id = match existing_base {
        Some(id) if is_pr_snapshot_valid(conn, id) => id,
        maybe_stale => {
            eprintln!(
                "Fetching merge base {}: {}",
                spec_name,
                truncate_chars(&resolved.base_html_url, 80)
            );
            let html = super::fetch_raw_html(&resolved.base_html_url).await?;
            let base_parsed = parse::parse_spec(&html, spec_name, base_url)?;
            if let Some(stale_id) = maybe_stale {
                write::delete_commit_snapshot(conn, stale_id)?;
            }
            let commit_date = chrono::Utc::now().to_rfc3339();
            let id = write::insert_snapshot(conn, spec_id, &resolved.merge_base_sha, &commit_date)?;
            write::insert_sections_bulk(conn, id, &base_parsed.sections)?;
            write::insert_refs_bulk(conn, id, &base_parsed.references)?;
            write::insert_idl_defs_bulk(conn, id, &base_parsed.idl_definitions)?;
            id
        }
    };

    // Fetch and parse PR pages.
    let pr_parsed = fetch_pr_pages(&resolved.pages, spec_name, base_url, pr_number).await?;
    let pr_sha = format!("pr:{}:{}", pr_number, resolved.head_sha);
    let commit_date = chrono::Utc::now().to_rfc3339();
    let page_paths: Vec<String> = resolved.pages.iter().map(|p| p.page_path.clone()).collect();
    let pr_snap_id = write::insert_pr_snapshot(
        conn,
        spec_id,
        &pr_sha,
        &commit_date,
        pr_number,
        &resolved.merge_base_sha,
        &page_paths,
    )?;
    write::insert_sections_bulk(conn, pr_snap_id, &pr_parsed.sections)?;
    write::insert_refs_bulk(conn, pr_snap_id, &pr_parsed.references)?;
    write::insert_idl_defs_bulk(conn, pr_snap_id, &pr_parsed.idl_definitions)?;

    Ok((pr_snap_id, base_snap_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A byte-index slice at 80 would panic when the offset lands inside a
    // multi-byte UTF-8 sequence; the char-based truncation must not.
    #[test]
    fn test_truncate_chars_multibyte_safe() {
        // 100 accented chars: byte length is 200, so slicing at byte 80 would
        // fall mid-character and panic.
        let url = "é".repeat(100);
        assert!(url.len() > 80 && url.chars().count() == 100);
        let truncated = truncate_chars(&url, 80);
        assert_eq!(truncated.chars().count(), 80);
        // Shorter-than-limit input is returned intact.
        assert_eq!(truncate_chars("abc", 80), "abc");
    }

    #[test]
    fn test_empty_pr_snapshot_not_treated_as_cached() {
        use crate::db;

        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg")
                .unwrap();

        write::insert_pr_snapshot(
            &conn,
            spec_id,
            "pr:99:deadbeef",
            "2026-01-01T00:00:00Z",
            99,
            "basesha",
            &[],
        )
        .unwrap();
        write::insert_snapshot(&conn, spec_id, "basesha", "2026-01-01T00:00:00Z").unwrap();

        let pr_snap_id: i64 = conn
            .query_row(
                "SELECT id FROM snapshots WHERE sha = 'pr:99:deadbeef'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!is_pr_snapshot_valid(&conn, pr_snap_id));
    }

    #[test]
    fn test_pr_snapshot_stale_parser_not_valid() {
        use crate::db;
        use crate::model::{ParsedSection, SectionType};

        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg")
                .unwrap();

        let pr_snap_id = write::insert_pr_snapshot(
            &conn,
            spec_id,
            "pr:99:deadbeef",
            "2026-01-01T00:00:00Z",
            99,
            "basesha",
            &[],
        )
        .unwrap();
        // Give it a section so the emptiness check is satisfied and parser
        // version is the only thing under test.
        write::insert_sections_bulk(
            &conn,
            pr_snap_id,
            &[ParsedSection {
                anchor: "sec-a".into(),
                title: Some("A".into()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
                number: None,
            }],
        )
        .unwrap();

        // Freshly inserted -> stamped with the current build -> valid.
        assert!(is_pr_snapshot_valid(&conn, pr_snap_id));

        // Produced by an older build -> stale -> not valid.
        conn.execute(
            "UPDATE snapshots SET index_version = ?1 WHERE id = ?2",
            ("0.0.0", pr_snap_id),
        )
        .unwrap();
        assert!(!is_pr_snapshot_valid(&conn, pr_snap_id));

        // Pre-upgrade rows (NULL index_version) -> not valid either.
        conn.execute(
            "UPDATE snapshots SET index_version = NULL WHERE id = ?1",
            [pr_snap_id],
        )
        .unwrap();
        assert!(!is_pr_snapshot_valid(&conn, pr_snap_id));
    }

    // A merge-base (commit) snapshot that fetched but parsed to zero sections
    // must be treated as invalid so it is re-fetched rather than cached forever;
    // reusing an empty base would make every PR section look newly-added in
    // `--diff`. The reuse guard shares `is_pr_snapshot_valid` for exactly this.
    #[test]
    fn test_empty_merge_base_snapshot_not_reused() {
        use crate::db;
        use crate::model::{ParsedSection, SectionType};

        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg")
                .unwrap();

        // A merge base is a plain commit snapshot (insert_snapshot, keyed by SHA).
        let base_snap_id =
            write::insert_snapshot(&conn, spec_id, "basesha", "2026-01-01T00:00:00Z").unwrap();

        // Empty (HTTP 200 but zero sections) -> not reusable.
        assert!(!is_pr_snapshot_valid(&conn, base_snap_id));

        // Once it has sections it becomes reusable.
        write::insert_sections_bulk(
            &conn,
            base_snap_id,
            &[ParsedSection {
                anchor: "sec-a".into(),
                title: Some("A".into()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
                number: None,
            }],
        )
        .unwrap();
        assert!(is_pr_snapshot_valid(&conn, base_snap_id));
    }

    // On resolve failure, the offline fallback serves a cached PR snapshot only
    // when BOTH the PR snapshot and its merge base are present — otherwise the
    // caller must surface the resolution error rather than a partial preview.
    #[test]
    fn test_cached_pr_fallback() {
        use crate::db;

        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg")
                .unwrap();

        // Nothing cached yet -> no fallback.
        assert!(cached_pr_fallback(&conn, spec_id, "HTML", 99)
            .unwrap()
            .is_none());

        // PR snapshot cached but its merge base missing -> still no fallback.
        let pr_snap_id = write::insert_pr_snapshot(
            &conn,
            spec_id,
            "pr:99:deadbeef",
            "2026-01-01T00:00:00Z",
            99,
            "basesha",
            &[],
        )
        .unwrap();
        assert!(cached_pr_fallback(&conn, spec_id, "HTML", 99)
            .unwrap()
            .is_none());

        // Both present -> serve the cached pair.
        let base_snap_id =
            write::insert_snapshot(&conn, spec_id, "basesha", "2026-01-01T00:00:00Z").unwrap();
        assert_eq!(
            cached_pr_fallback(&conn, spec_id, "HTML", 99).unwrap(),
            Some((pr_snap_id, base_snap_id))
        );
    }

    #[test]
    fn test_merge_parsed_specs() {
        use crate::model::{
            ParsedIdlDefinition, ParsedReference, ParsedSection, ParsedSpec, SectionType,
        };

        let idl_b = || ParsedIdlDefinition {
            anchor: "dom-window".into(),
            name: "Window".into(),
            owner: None,
            kind: "interface".into(),
            canonical_name: "Window".into(),
            idl_text: None,
        };

        let spec_a = ParsedSpec {
            sections: vec![ParsedSection {
                anchor: "sec-a".into(),
                title: Some("A".into()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
                number: None,
            }],
            references: vec![],
            idl_definitions: vec![ParsedIdlDefinition {
                anchor: "dom-document".into(),
                name: "Document".into(),
                owner: None,
                kind: "interface".into(),
                canonical_name: "Document".into(),
                idl_text: None,
            }],
        };
        let spec_b = ParsedSpec {
            sections: vec![ParsedSection {
                anchor: "sec-b".into(),
                title: Some("B".into()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
                number: None,
            }],
            references: vec![ParsedReference::prose("sec-b", "DOM", "concept-tree")],
            idl_definitions: vec![idl_b()],
        };

        // A third page repeats sec-b (and its reference and IDL), as a shared
        // anchor appearing on two preview pages would.
        let spec_b_dup = ParsedSpec {
            sections: vec![ParsedSection {
                anchor: "sec-b".into(),
                title: Some("B".into()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
                number: None,
            }],
            references: vec![ParsedReference::prose("sec-b", "DOM", "concept-tree")],
            idl_definitions: vec![idl_b()],
        };

        let merged = merge_parsed_specs(vec![spec_a, spec_b, spec_b_dup]);
        // Duplicate section, reference, AND IDL definition are all dropped; the
        // two distinct IDL definitions (Document, Window) survive.
        assert_eq!(merged.sections.len(), 2);
        assert_eq!(merged.references.len(), 1);
        assert_eq!(merged.idl_definitions.len(), 2);
    }
}
