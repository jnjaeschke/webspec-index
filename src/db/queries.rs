// Query operations on the database
use crate::model::{ParsedSection, SectionType};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

type RepoShaCache = Option<(String, DateTime<Utc>, DateTime<Utc>)>;

/// Get the snapshot for a spec by name (each spec has at most one snapshot)
pub fn get_snapshot(conn: &Connection, spec_name: &str) -> Result<Option<i64>> {
    let result = conn.query_row(
        "SELECT s.id FROM snapshots s
         JOIN specs sp ON s.spec_id = sp.id
         WHERE sp.name = ?1",
        [spec_name],
        |row| row.get(0),
    );

    match result {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get the cached latest SHA for a GitHub repo, if present and fresh enough for the caller to decide.
/// Returns (sha, commit_date, checked_at) if a cache entry exists.
pub fn get_repo_sha_cache(conn: &Connection, repo: &str) -> Result<RepoShaCache> {
    let result = conn.query_row(
        "SELECT sha, commit_date, checked_at FROM repo_version_cache WHERE repo = ?1",
        [repo],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    );

    match result {
        Ok((sha, commit_date_str, checked_at_str)) => {
            let commit_date = DateTime::parse_from_rfc3339(&commit_date_str)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| {
                    rusqlite::Error::InvalidColumnType(
                        1,
                        e.to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;
            let checked_at = DateTime::parse_from_rfc3339(&checked_at_str)
                .map(|d| d.with_timezone(&Utc))
                .map_err(|e| {
                    rusqlite::Error::InvalidColumnType(
                        2,
                        e.to_string(),
                        rusqlite::types::Type::Text,
                    )
                })?;
            Ok(Some((sha, commit_date, checked_at)))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get a snapshot by spec name and SHA
pub fn get_snapshot_by_sha(conn: &Connection, spec_name: &str, sha: &str) -> Result<Option<i64>> {
    let result = conn.query_row(
        "SELECT s.id FROM snapshots s
         JOIN specs sp ON s.spec_id = sp.id
         WHERE sp.name = ?1 AND s.sha = ?2",
        (spec_name, sha),
        |row| row.get(0),
    );

    match result {
        Ok(id) => Ok(Some(id)),
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
         WHERE r.to_spec = ?1 AND r.to_anchor = ?2",
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
    limit: usize,
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
    limit: usize,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, write};

    fn setup_test_data(conn: &Connection) -> Result<i64> {
        let spec_id =
            write::insert_or_get_spec(conn, "HTML", "https://html.spec.whatwg.org", "whatwg")?;
        let snapshot_id = write::insert_snapshot(conn, spec_id, "abc123", "2026-01-01T00:00:00Z")?;

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
}
