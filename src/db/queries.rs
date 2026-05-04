// Query operations on the database
use crate::model::{ParsedSection, PrDiffEntry, SectionType};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct UpdateCheckState {
    pub last_checked: DateTime<Utc>,
    pub last_indexed: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
}

/// List all cached PR snapshots with their section counts.
/// Returns (spec_name, pr_number, sha, indexed_at, section_count).
pub fn list_pr_snapshots(conn: &Connection) -> Result<Vec<(String, i64, String, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT sp.name, s.pr_number, s.sha, s.indexed_at,
                (SELECT COUNT(*) FROM sections sec WHERE sec.snapshot_id = s.id)
         FROM snapshots s
         JOIN specs sp ON s.spec_id = sp.id
         WHERE s.pr_number IS NOT NULL
         ORDER BY sp.name, s.pr_number",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get a PR snapshot for a spec by name and PR number.
/// Returns (snapshot_id, merge_base_sha) if found.
pub fn get_pr_snapshot(
    conn: &Connection,
    spec_name: &str,
    pr_number: i64,
) -> Result<Option<(i64, String)>> {
    let result = conn.query_row(
        "SELECT s.id, s.merge_base_sha FROM snapshots s
         JOIN specs sp ON s.spec_id = sp.id
         WHERE sp.name = ?1 AND s.pr_number = ?2",
        (spec_name, pr_number),
        |row| Ok((row.get(0)?, row.get::<_, String>(1)?)),
    );
    match result {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get a commit snapshot by spec_id and exact SHA.
/// Used to find cached merge base snapshots.
pub fn get_commit_snapshot(
    conn: &Connection,
    spec_id: i64,
    sha: &str,
) -> Result<Option<i64>> {
    let result = conn.query_row(
        "SELECT id FROM snapshots
         WHERE spec_id = ?1 AND sha = ?2 AND pr_number IS NULL",
        (spec_id, sha),
        |row| row.get(0),
    );
    match result {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get the list of page paths stored for a PR snapshot.
pub fn get_pr_pages(conn: &Connection, snapshot_id: i64) -> Result<Vec<String>> {
    let pages: Option<String> = conn.query_row(
        "SELECT pr_pages FROM snapshots WHERE id = ?1",
        [snapshot_id],
        |row| row.get(0),
    )?;
    Ok(pages
        .map(|s| s.split(',').filter(|p| !p.is_empty()).map(|p| p.to_string()).collect())
        .unwrap_or_default())
}

/// Get the snapshot for a spec by name (each spec has at most one snapshot)
pub fn get_snapshot(conn: &Connection, spec_name: &str) -> Result<Option<i64>> {
    let result = conn.query_row(
        "SELECT s.id FROM snapshots s
         JOIN specs sp ON s.spec_id = sp.id
         WHERE sp.name = ?1 AND s.pr_number IS NULL AND s.sha LIKE 'hash:%'",
        [spec_name],
        |row| row.get(0),
    );

    match result {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get canonical spec metadata by name (case-insensitive).
/// Returns (name, base_url, provider).
pub fn get_spec_meta(
    conn: &Connection,
    spec_name: &str,
) -> Result<Option<(String, String, String)>> {
    let row = conn.query_row(
        "SELECT name, base_url, provider
         FROM specs
         WHERE LOWER(name) = LOWER(?1)
         LIMIT 1",
        [spec_name],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    );

    match row {
        Ok(meta) => Ok(Some(meta)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List all indexed/discovered specs as (name, base_url, provider).
pub fn list_specs(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT name, base_url, provider
         FROM specs
         ORDER BY name",
    )?;

    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get sync metadata for a spec from update_checks.
pub fn get_update_check(conn: &Connection, spec_id: i64) -> Result<Option<UpdateCheckState>> {
    let row = conn.query_row(
        "SELECT last_checked, last_indexed, content_hash
         FROM update_checks
         WHERE spec_id = ?1",
        [spec_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    );

    match row {
        Ok((checked, indexed, content_hash)) => {
            let last_checked = DateTime::parse_from_rfc3339(&checked)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| {
                    rusqlite::Error::InvalidColumnType(
                        0,
                        e.to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;

            let last_indexed = match indexed {
                Some(value) => Some(
                    DateTime::parse_from_rfc3339(&value)
                        .map(|d| d.with_timezone(&Utc))
                        .map_err(|e| {
                            rusqlite::Error::InvalidColumnType(
                                1,
                                e.to_string(),
                                rusqlite::types::Type::Text,
                            )
                        })?,
                ),
                None => None,
            };

            Ok(Some(UpdateCheckState {
                last_checked,
                last_indexed,
                content_hash,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get a section by snapshot ID and anchor
pub fn get_section(
    conn: &Connection,
    snapshot_id: i64,
    anchor: &str,
) -> Result<Option<ParsedSection>> {
    let result = conn.query_row(
        "SELECT anchor, title, content_text, section_type, parent_anchor, prev_anchor, next_anchor, depth
         FROM sections
         WHERE snapshot_id = ?1 AND anchor = ?2",
        (snapshot_id, anchor),
        |row| {
            Ok(ParsedSection {
                anchor: row.get(0)?,
                title: row.get(1)?,
                content_text: row.get(2)?,
                section_type: row.get::<_, String>(3)?.parse::<SectionType>()
                    .map_err(|_| rusqlite::Error::InvalidColumnType(3, "section_type".to_string(), rusqlite::types::Type::Text))?,
                parent_anchor: row.get(4)?,
                prev_anchor: row.get(5)?,
                next_anchor: row.get(6)?,
                depth: row.get(7)?,
            })
        },
    );

    match result {
        Ok(section) => Ok(Some(section)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get child sections (sections with this as parent)
pub fn get_children(
    conn: &Connection,
    snapshot_id: i64,
    parent_anchor: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT anchor, title FROM sections
         WHERE snapshot_id = ?1 AND parent_anchor = ?2
         ORDER BY rowid",
    )?;

    let children = stmt
        .query_map((snapshot_id, parent_anchor), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(children)
}

/// Get outgoing references from a section
pub fn get_outgoing_refs(
    conn: &Connection,
    snapshot_id: i64,
    from_anchor: &str,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT to_spec, to_anchor FROM refs
         WHERE snapshot_id = ?1 AND from_anchor = ?2",
    )?;

    let refs = stmt
        .query_map((snapshot_id, from_anchor), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(refs)
}

/// Get incoming references to a section
/// Returns (from_spec, from_anchor) tuples
/// Searches across all indexed specs to find cross-spec refs
pub fn get_incoming_refs(
    conn: &Connection,
    to_spec: &str,
    to_anchor: &str,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT sp.name, r.from_anchor FROM refs r
         JOIN snapshots sn ON r.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE r.to_spec = ?1 AND r.to_anchor = ?2 AND sn.pr_number IS NULL AND sn.sha LIKE 'hash:%'",
    )?;

    let refs = stmt
        .query_map((to_spec, to_anchor), |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(refs)
}

/// Search sections using FTS5
#[cfg(test)]
pub fn search_sections(
    conn: &Connection,
    query: &str,
    spec_filter: Option<&str>,
    limit: u32,
) -> Result<Vec<(String, String, Option<String>)>> {
    let sql = if let Some(_spec) = spec_filter {
        "SELECT s.anchor, sp.name, snippet(sections_fts, 2, '<mark>', '</mark>', '...', 64)
         FROM sections_fts
         JOIN sections s ON sections_fts.rowid = s.id
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE sections_fts MATCH ?1 AND sp.name = ?2          LIMIT ?3"
    } else {
        "SELECT s.anchor, sp.name, snippet(sections_fts, 2, '<mark>', '</mark>', '...', 64)
         FROM sections_fts
         JOIN sections s ON sections_fts.rowid = s.id
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE sections_fts MATCH ?1          LIMIT ?2"
    };

    let mut stmt = conn.prepare(sql)?;

    let results = if let Some(spec) = spec_filter {
        stmt.query_map((query, spec, limit), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map((query, limit), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    };

    Ok(results)
}

/// Find anchors matching a pattern
#[cfg(test)]
pub fn find_anchors(
    conn: &Connection,
    pattern: &str,
    spec_filter: Option<&str>,
    limit: u32,
) -> Result<Vec<(String, String)>> {
    let sql = if let Some(_spec) = spec_filter {
        "SELECT s.anchor, sp.name FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE s.anchor LIKE ?1 AND sp.name = ?2          LIMIT ?3"
    } else {
        "SELECT s.anchor, sp.name FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE s.anchor LIKE ?1          LIMIT ?2"
    };

    let mut stmt = conn.prepare(sql)?;

    let results = if let Some(spec) = spec_filter {
        stmt.query_map((pattern, spec, limit), |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map((pattern, limit), |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(results)
}

/// List all headings in a spec
pub fn list_headings(conn: &Connection, snapshot_id: i64) -> Result<Vec<ParsedSection>> {
    let mut stmt = conn.prepare(
        "SELECT anchor, title, content_text, section_type, parent_anchor, prev_anchor, next_anchor, depth
         FROM sections
         WHERE snapshot_id = ?1 AND section_type = 'heading'
         ORDER BY rowid",
    )?;

    let sections = stmt
        .query_map([snapshot_id], |row| {
            Ok(ParsedSection {
                anchor: row.get(0)?,
                title: row.get(1)?,
                content_text: row.get(2)?,
                section_type: row
                    .get::<_, String>(3)?
                    .parse::<SectionType>()
                    .map_err(|_| {
                        rusqlite::Error::InvalidColumnType(
                            3,
                            "section_type".to_string(),
                            rusqlite::types::Type::Text,
                        )
                    })?,
                parent_anchor: row.get(4)?,
                prev_anchor: row.get(5)?,
                next_anchor: row.get(6)?,
                depth: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(sections)
}

fn normalize_content(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn content_eq(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => normalize_content(a) == normalize_content(b),
        _ => false,
    }
}

/// Compute a diff between a PR snapshot and its merge base snapshot.
/// Returns entries for sections that were added or modified in the PR.
pub fn compute_pr_diff(
    conn: &Connection,
    pr_snapshot_id: i64,
    base_snapshot_id: i64,
) -> Result<Vec<PrDiffEntry>> {
    let mut pr_stmt = conn.prepare(
        "SELECT anchor, title, content_text FROM sections WHERE snapshot_id = ?1",
    )?;
    let pr_sections: HashMap<String, (Option<String>, Option<String>)> = pr_stmt
        .query_map([pr_snapshot_id], |row| {
            Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;

    let mut diffs = Vec::new();

    for (anchor, (title, new_content)) in &pr_sections {
        match get_section(conn, base_snapshot_id, anchor)? {
            Some(base_section) => {
                if !content_eq(base_section.content_text.as_deref(), new_content.as_deref()) {
                    diffs.push(PrDiffEntry {
                        anchor: anchor.clone(),
                        title: title.clone(),
                        change_type: "modified".to_string(),
                        old_content: base_section.content_text,
                        new_content: new_content.clone(),
                    });
                }
            }
            None => {
                diffs.push(PrDiffEntry {
                    anchor: anchor.clone(),
                    title: title.clone(),
                    change_type: "added".to_string(),
                    old_content: None,
                    new_content: new_content.clone(),
                });
            }
        }
    }

    diffs.sort_by(|a, b| a.anchor.cmp(&b.anchor));
    Ok(diffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, write};

    fn setup_test_data(conn: &Connection) -> Result<i64> {
        let spec_id =
            write::insert_or_get_spec(conn, "HTML", "https://html.spec.whatwg.org", "whatwg")?;
        let snapshot_id = write::insert_snapshot(conn, spec_id, "hash:abc123", "2026-01-01T00:00:00Z")?;

        let sections = vec![
            ParsedSection {
                anchor: "intro".to_string(),
                title: Some("Introduction".to_string()),
                content_text: Some("This is an introduction to HTML".to_string()),
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: Some("details".to_string()),
                depth: Some(2),
            },
            ParsedSection {
                anchor: "details".to_string(),
                title: Some("Details".to_string()),
                content_text: Some("More details about the specification".to_string()),
                section_type: SectionType::Heading,
                parent_anchor: Some("intro".to_string()),
                prev_anchor: Some("intro".to_string()),
                next_anchor: None,
                depth: Some(3),
            },
        ];

        write::insert_sections_bulk(conn, snapshot_id, &sections)?;

        Ok(snapshot_id)
    }

    #[test]
    fn test_get_snapshot() {
        let conn = db::open_test_db().unwrap();
        let snapshot_id = setup_test_data(&conn).unwrap();

        let result = get_snapshot(&conn, "HTML").unwrap();
        assert_eq!(result, Some(snapshot_id));

        let result = get_snapshot(&conn, "NonExistent").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_get_spec_meta_and_list_specs() {
        let conn = db::open_test_db().unwrap();
        setup_test_data(&conn).unwrap();

        let meta = get_spec_meta(&conn, "html").unwrap().unwrap();
        assert_eq!(meta.0, "HTML");
        assert_eq!(meta.1, "https://html.spec.whatwg.org");
        assert_eq!(meta.2, "whatwg");

        let specs = list_specs(&conn).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].0, "HTML");
    }

    #[test]
    fn test_get_section() {
        let conn = db::open_test_db().unwrap();
        let snapshot_id = setup_test_data(&conn).unwrap();

        let section = get_section(&conn, snapshot_id, "intro").unwrap();
        assert!(section.is_some());
        let section = section.unwrap();
        assert_eq!(section.anchor, "intro");
        assert_eq!(section.title, Some("Introduction".to_string()));

        let section = get_section(&conn, snapshot_id, "nonexistent").unwrap();
        assert!(section.is_none());
    }

    #[test]
    fn test_get_children() {
        let conn = db::open_test_db().unwrap();
        let snapshot_id = setup_test_data(&conn).unwrap();

        let children = get_children(&conn, snapshot_id, "intro").unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].0, "details");
    }

    #[test]
    fn test_search_sections() {
        let conn = db::open_test_db().unwrap();
        setup_test_data(&conn).unwrap();

        let results = search_sections(&conn, "introduction", None, 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "intro");
    }

    #[test]
    fn test_find_anchors() {
        let conn = db::open_test_db().unwrap();
        setup_test_data(&conn).unwrap();

        let results = find_anchors(&conn, "intro%", None, 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "intro");
    }

    #[test]
    fn test_list_headings() {
        let conn = db::open_test_db().unwrap();
        let snapshot_id = setup_test_data(&conn).unwrap();

        let headings = list_headings(&conn, snapshot_id).unwrap();
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].anchor, "intro");
        assert_eq!(headings[1].anchor, "details");
    }

    #[test]
    fn test_get_pr_snapshot() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        // No PR snapshot yet
        let result = get_pr_snapshot(&conn, "HTML", 12345).unwrap();
        assert!(result.is_none());

        // Insert PR snapshot
        let snap_id = write::insert_pr_snapshot(
            &conn, spec_id, "pr-sha", "2026-01-01T00:00:00Z", 12345, "base-sha", &[],
        ).unwrap();

        let result = get_pr_snapshot(&conn, "HTML", 12345).unwrap();
        assert_eq!(result, Some((snap_id, "base-sha".to_string())));
    }

    #[test]
    fn test_get_commit_snapshot() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        // Insert a commit snapshot (regular snapshot with a real SHA)
        let snap_id = write::insert_snapshot(
            &conn, spec_id, "abc123full", "2026-01-01T00:00:00Z",
        ).unwrap();

        let result = get_commit_snapshot(&conn, spec_id, "abc123full").unwrap();
        assert_eq!(result, Some(snap_id));

        let result = get_commit_snapshot(&conn, spec_id, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_update_check() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg")
                .unwrap();

        write::record_update_check(
            &conn,
            spec_id,
            "2026-01-01T00:00:00Z",
            Some("2026-01-01T00:00:00Z"),
            Some("abc123"),
        )
        .unwrap();

        let state = get_update_check(&conn, spec_id).unwrap().unwrap();
        assert_eq!(state.last_checked.to_rfc3339(), "2026-01-01T00:00:00+00:00");
        assert_eq!(
            state.last_indexed.as_ref().unwrap().to_rfc3339(),
            "2026-01-01T00:00:00+00:00"
        );
        assert_eq!(state.content_hash.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_compute_pr_diff() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        let base_id = write::insert_snapshot(&conn, spec_id, "base", "2026-01-01T00:00:00Z").unwrap();
        write::insert_sections_bulk(&conn, base_id, &[
            ParsedSection { anchor: "sec-a".into(), title: Some("A".into()),
                content_text: Some("Original A".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
            ParsedSection { anchor: "sec-b".into(), title: Some("B".into()),
                content_text: Some("Original B".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
        ]).unwrap();

        let pr_id = write::insert_pr_snapshot(
            &conn, spec_id, "pr-sha", "2026-01-01T00:00:00Z", 99, "base", &[],
        ).unwrap();
        write::insert_sections_bulk(&conn, pr_id, &[
            ParsedSection { anchor: "sec-a".into(), title: Some("A".into()),
                content_text: Some("Modified A".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
            ParsedSection { anchor: "sec-d".into(), title: Some("D".into()),
                content_text: Some("New D".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
        ]).unwrap();

        let diff = compute_pr_diff(&conn, pr_id, base_id).unwrap();
        assert_eq!(diff.len(), 2);

        let modified: Vec<_> = diff.iter().filter(|d| d.change_type == "modified").collect();
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].anchor, "sec-a");

        let added: Vec<_> = diff.iter().filter(|d| d.change_type == "added").collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].anchor, "sec-d");
    }

    #[test]
    fn test_compute_pr_diff_ignores_whitespace_only_changes() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        let base_id = write::insert_snapshot(&conn, spec_id, "hash:base2", "2026-01-01T00:00:00Z").unwrap();
        write::insert_sections_bulk(&conn, base_id, &[
            ParsedSection { anchor: "sec-a".into(), title: Some("A".into()),
                content_text: Some("Hello  world\n\nfoo".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
            ParsedSection { anchor: "sec-b".into(), title: Some("B".into()),
                content_text: Some("Real content B".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
        ]).unwrap();

        let pr_id = write::insert_pr_snapshot(
            &conn, spec_id, "pr-sha2", "2026-01-01T00:00:00Z", 98, "hash:base2", &[],
        ).unwrap();
        write::insert_sections_bulk(&conn, pr_id, &[
            ParsedSection { anchor: "sec-a".into(), title: Some("A".into()),
                content_text: Some("Hello world\n\nfoo".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
            ParsedSection { anchor: "sec-b".into(), title: Some("B".into()),
                content_text: Some("Modified content B".into()), section_type: SectionType::Heading,
                parent_anchor: None, prev_anchor: None, next_anchor: None, depth: Some(2) },
        ]).unwrap();

        let diff = compute_pr_diff(&conn, pr_id, base_id).unwrap();
        assert_eq!(diff.len(), 1);
        assert_eq!(diff[0].anchor, "sec-b");
        assert_eq!(diff[0].change_type, "modified");
    }
}
