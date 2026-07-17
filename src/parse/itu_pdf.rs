// ITU-T Recommendation PDF parsing
//
// ITU-T Recommendations are distributed as PDF only (no free full-text
// HTML, unlike RFCs). This module derives ParsedSection entries from a
// PDF's outline/bookmark tree (clause structure) and per-page body text.

use crate::model::{ParsedSection, SectionType};
use anyhow::{Context, Result};

/// Parse an ITU-T Recommendation PDF into structured sections, one per
/// outline/bookmark entry. The clause number (e.g. "D.3.28", "7.4.3.1") is
/// the leading whitespace-delimited token of the outline title; the anchor
/// therefore matches how these clauses are normally cited in prose.
pub fn parse_itu_pdf(bytes: &[u8]) -> Result<Vec<ParsedSection>> {
    let doc = lopdf::Document::load_mem(bytes).context("Failed to parse ITU-T PDF")?;
    let toc = doc.get_toc().map_err(|e| {
        anyhow::anyhow!(
            "ITU-T PDF has no outline/bookmark tree (needed to derive clause anchors): {}",
            e
        )
    })?;
    if toc.toc.is_empty() {
        anyhow::bail!("ITU-T PDF outline is empty; cannot derive any clause anchors");
    }

    let pages = pdf_extract::extract_text_from_mem_by_pages(bytes)
        .map_err(|e| anyhow::anyhow!("Failed to extract text from ITU-T PDF: {}", e))?;

    let mut sections = Vec::with_capacity(toc.toc.len());
    for (i, entry) in toc.toc.iter().enumerate() {
        let (anchor, title) = split_anchor_title(&entry.title);
        if anchor.is_empty() {
            continue;
        }

        let end_page = toc
            .toc
            .get(i + 1)
            .map(|next| next.page)
            .unwrap_or(pages.len() + 1);
        let content_text = extract_page_range_text(&pages, entry.page, end_page);

        sections.push(ParsedSection {
            anchor,
            title: if title.is_empty() { None } else { Some(title) },
            content_text,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(entry.level.min(255) as u8),
        });
    }

    Ok(sections)
}

/// Split an outline title into (anchor, display title): the leading
/// whitespace-delimited token is the clause number, the rest is the title.
/// "D.3.28 Mastering display..." -> ("D.3.28", "Mastering display...").
/// Deliberately does not special-case Annex nodes (e.g. "D  Annex D  ...")
/// — the same rule already yields the correct anchor for those.
fn split_anchor_title(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    match trimmed.split_once(char::is_whitespace) {
        Some((anchor, rest)) => (anchor.to_string(), rest.trim().to_string()),
        None => (trimmed.to_string(), String::new()),
    }
}

/// Extract and whitespace-collapse the text of pages `[start_page, end_page)`
/// (1-indexed, matching PDF page numbers). A clause's extracted text may
/// include a trailing/leading fragment of a neighboring clause when they
/// share a page — an accepted approximation of page-based slicing.
fn extract_page_range_text(pages: &[String], start_page: usize, end_page: usize) -> Option<String> {
    let start_idx = start_page.saturating_sub(1).min(pages.len());
    let end_idx = end_page.saturating_sub(1).min(pages.len());
    if start_idx >= end_idx {
        return None;
    }
    let collapsed = collapse_whitespace(&pages[start_idx..end_idx].join(" "));
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

/// Collapse all whitespace (including newlines and the double-spaces
/// `pdf-extract` produces for justified text) into single spaces.
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Builds a minimal synthetic PDF in memory: one page per string in
    // `pages`, and an outline built from `bookmarks` — each entry is
    // (title, page_index, parent_index_into_this_slice).
    fn build_test_pdf(pages: &[&str], bookmarks: &[(&str, usize, Option<usize>)]) -> Vec<u8> {
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

        let page_ids: Vec<(u32, u16)> = pages
            .iter()
            .map(|text| {
                let content = Content {
                    operations: vec![
                        Operation::new("BT", vec![]),
                        Operation::new("Tf", vec!["F1".into(), 12.into()]),
                        Operation::new("Td", vec![72.into(), 700.into()]),
                        Operation::new("Tj", vec![Object::string_literal(*text)]),
                        Operation::new("ET", vec![]),
                    ],
                };
                let content_id =
                    doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
                doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "Contents" => content_id,
                    "Resources" => resources_id,
                })
            })
            .collect();

        let kids: Vec<Object> = page_ids.iter().map(|id| (*id).into()).collect();
        let pages_dict = dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => page_ids.len() as i64,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut bookmark_ids: Vec<u32> = Vec::with_capacity(bookmarks.len());
        for (title, page_idx, parent_idx) in bookmarks {
            let bm = Bookmark::new(
                (*title).to_string(),
                [0.0, 0.0, 0.0],
                0,
                page_ids[*page_idx],
            );
            let parent = parent_idx.map(|i| bookmark_ids[i]);
            bookmark_ids.push(doc.add_bookmark(bm, parent));
        }

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
    fn test_parse_itu_pdf_basic_structure() {
        let bytes = build_test_pdf(
            &[
                "Scope page text.",
                "Clause one body text about widgets.",
                "Clause two body text about gadgets.",
            ],
            &[
                ("1 Scope", 0, None),
                ("1.1 Widgets", 1, Some(0)),
                ("1.2 Gadgets", 2, Some(0)),
            ],
        );

        let sections = parse_itu_pdf(&bytes).unwrap();
        assert_eq!(sections.len(), 3);

        assert_eq!(sections[0].anchor, "1");
        assert_eq!(sections[0].title, Some("Scope".to_string()));
        assert_eq!(sections[0].depth, Some(1));

        assert_eq!(sections[1].anchor, "1.1");
        assert_eq!(sections[1].title, Some("Widgets".to_string()));
        assert_eq!(sections[1].depth, Some(2));
        let content1 = sections[1].content_text.as_ref().unwrap();
        assert!(content1.contains("widgets"));

        assert_eq!(sections[2].anchor, "1.2");
        assert_eq!(sections[2].title, Some("Gadgets".to_string()));
        let content2 = sections[2].content_text.as_ref().unwrap();
        assert!(content2.contains("gadgets"));
    }

    #[test]
    fn test_parse_itu_pdf_no_outline_errors() {
        let bytes = build_test_pdf(&["Just a page."], &[]);
        let result = parse_itu_pdf(&bytes);
        assert!(
            result.is_err(),
            "a PDF with no outline must fail loudly, not return zero sections silently"
        );
    }

    #[test]
    fn test_split_anchor_title_simple() {
        let (anchor, title) =
            split_anchor_title("D.3.28 Mastering display colour volume SEI message semantics");
        assert_eq!(anchor, "D.3.28");
        assert_eq!(
            title,
            "Mastering display colour volume SEI message semantics"
        );
    }

    #[test]
    fn test_split_anchor_title_annex_double_space() {
        // Real ITU-T H.265 outline title for the Annex D node: no
        // special-casing needed, the leading token is still correct.
        let (anchor, title) =
            split_anchor_title("D  Annex D  Supplemental enhancement information");
        assert_eq!(anchor, "D");
        assert_eq!(title, "Annex D  Supplemental enhancement information");
    }

    #[test]
    fn test_split_anchor_title_no_remainder() {
        let (anchor, title) = split_anchor_title("D.3.28");
        assert_eq!(anchor, "D.3.28");
        assert_eq!(title, "");
    }

    #[test]
    fn test_collapse_whitespace() {
        assert_eq!(collapse_whitespace("a   b\n\nc\t d"), "a b c d");
    }
}
