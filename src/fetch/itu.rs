// PDF-specific sync/fetch logic for ITU-T Recommendations.
//
// Mirrors the shape of the HTML sync path in `fetch::mod`, but fetches raw
// PDF bytes and parses them via `parse::itu_pdf` instead of
// `parse::parse_spec`. ITU-T PDFs have neither hyperlinks nor WebIDL, so
// references and IDL definitions are always inserted empty.

use crate::db::{queries, write};
use crate::parse;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

const USER_AGENT: &str = concat!("webspec-index/", env!("CARGO_PKG_VERSION"));
const PROVIDER_NAME: &str = "itu";

async fn fetch_pdf_bytes(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("Failed to fetch {}: HTTP {}", url, response.status());
    }
    Ok(response.bytes().await?.to_vec())
}

#[allow(clippy::too_many_arguments)]
fn sync_from_pdf(
    conn: &Connection,
    spec_id: i64,
    spec_name: &str,
    base_url: &str,
    bytes: Vec<u8>,
    previous_snapshot_id: Option<i64>,
    state: Option<queries::UpdateCheckState>,
    now: &DateTime<Utc>,
) -> Result<(i64, bool)> {
    let content_hash = super::hash_bytes(&bytes);

    if let (Some(snapshot_id), Some(state)) = (previous_snapshot_id, state.as_ref()) {
        let content_unchanged = state.content_hash.as_deref() == Some(content_hash.as_str());
        if content_unchanged && state.index_version.as_deref() == Some(parse::INDEX_VERSION) {
            let checked = now.to_rfc3339();
            let indexed = state.last_indexed.as_ref().map(|t| t.to_rfc3339());
            write::record_update_check(
                conn,
                spec_id,
                &checked,
                indexed.as_deref(),
                Some(&content_hash),
                Some(parse::INDEX_VERSION),
            )?;
            return Ok((snapshot_id, false));
        }
    }

    let sections = parse::itu_pdf::parse_itu_pdf(&bytes)?;
    let sections = parse::sections::build_section_tree(sections);
    write::delete_spec_data(conn, spec_id)?;

    let synthetic_sha = format!("hash:{content_hash}");
    let commit_date = now.to_rfc3339();
    let spec_id_reloaded = write::insert_or_get_spec(conn, spec_name, base_url, PROVIDER_NAME)?;
    let snapshot_id = write::insert_snapshot(conn, spec_id_reloaded, &synthetic_sha, &commit_date)?;
    write::insert_sections_bulk(conn, snapshot_id, &sections)?;
    write::insert_refs_bulk(conn, snapshot_id, &[])?;
    write::insert_idl_defs_bulk(conn, snapshot_id, &[])?;

    let checked = now.to_rfc3339();
    write::record_update_check(
        conn,
        spec_id_reloaded,
        &checked,
        Some(&checked),
        Some(&content_hash),
        Some(parse::INDEX_VERSION),
    )?;
    Ok((snapshot_id, true))
}

/// PDF equivalent of `fetch::sync_known_spec`: same freshness/fallback
/// policy, but fetches PDF bytes and parses via `parse::itu_pdf`.
pub async fn sync_known_spec_pdf(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
    force: bool,
    allow_fallback: bool,
) -> Result<(i64, bool)> {
    let spec_id = write::insert_or_get_spec(conn, spec_name, base_url, PROVIDER_NAME)?;
    let previous_snapshot_id = queries::get_snapshot(conn, spec_name)?;
    let state = queries::get_update_check(conn, spec_id)?;
    let now = Utc::now();

    if !force {
        if let (Some(snapshot_id), Some(sync_state)) = (previous_snapshot_id, state.as_ref()) {
            if super::cache_is_current(sync_state, &now) {
                return Ok((snapshot_id, false));
            }
        }
    }

    match fetch_pdf_bytes(base_url).await {
        Ok(bytes) => sync_from_pdf(
            conn,
            spec_id,
            spec_name,
            base_url,
            bytes,
            previous_snapshot_id,
            state,
            &now,
        ),
        Err(e) => match previous_snapshot_id {
            Some(snapshot_id) if allow_fallback && !force => Ok((snapshot_id, false)),
            _ => Err(e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn build_minimal_pdf(clause_title: &str, page_text: &str) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Bookmark, Document, Object, Stream};

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(page_text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
        });
        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let bm = Bookmark::new(clause_title.to_string(), [0.0, 0.0, 0.0], 0, page_id);
        doc.add_bookmark(bm, None);
        if let Some(outline_id) = doc.build_outline() {
            if let Ok(catalog) = doc.catalog_mut() {
                catalog.set("Outlines", outline_id);
            }
        }
        doc.compress();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn test_sync_from_pdf_first_index() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "H.TEST", "https://example.test/rec.pdf", "itu")
                .unwrap();
        let bytes = build_minimal_pdf("1 Scope", "This clause defines the scope.");
        let now = fixed_now();

        let (snapshot_id, updated) = sync_from_pdf(
            &conn,
            spec_id,
            "H.TEST",
            "https://example.test/rec.pdf",
            bytes,
            None,
            None,
            &now,
        )
        .unwrap();
        assert!(updated);

        let section = queries::get_section(&conn, snapshot_id, "1")
            .unwrap()
            .unwrap();
        assert_eq!(section.title, Some("Scope".to_string()));
    }

    #[test]
    fn test_sync_from_pdf_skips_reparse_when_unchanged() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "H.TEST", "https://example.test/rec.pdf", "itu")
                .unwrap();
        let bytes = build_minimal_pdf("1 Scope", "This clause defines the scope.");
        let now = fixed_now();

        let (snap1, _) = sync_from_pdf(
            &conn,
            spec_id,
            "H.TEST",
            "https://example.test/rec.pdf",
            bytes.clone(),
            None,
            None,
            &now,
        )
        .unwrap();

        let state = queries::get_update_check(&conn, spec_id).unwrap();
        let (snap2, updated2) = sync_from_pdf(
            &conn,
            spec_id,
            "H.TEST",
            "https://example.test/rec.pdf",
            bytes,
            Some(snap1),
            state,
            &now,
        )
        .unwrap();
        assert!(
            !updated2,
            "identical PDF bytes + current index version should skip re-parsing"
        );
        assert_eq!(snap1, snap2);
    }

    #[tokio::test]
    #[ignore = "hits the live network — run manually with `cargo test --lib fetch::itu:: -- --ignored`"]
    async fn itu_live_h265_d328_end_to_end() {
        let (canonical, pdf_url) = crate::itu::discover_spec("H.265")
            .await
            .unwrap()
            .expect("H.265 should resolve to a real edition");
        assert_eq!(canonical, "H.265");

        let conn = db::open_test_db().unwrap();
        let (snapshot_id, updated) = sync_known_spec_pdf(&conn, &canonical, &pdf_url, false, false)
            .await
            .unwrap();
        assert!(updated);

        let section = queries::get_section(&conn, snapshot_id, "D.3.28")
            .unwrap()
            .expect("D.3.28 should be indexed");
        assert!(
            section
                .title
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("mastering display"),
            "unexpected title: {:?}",
            section.title
        );
    }
}
