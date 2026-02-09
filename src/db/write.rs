// Write operations on the database
use crate::model::{ParsedReference, ParsedSection};
use anyhow::Result;
use rusqlite::Connection;

/// Insert or get a spec, returning its ID
/// Uses INSERT OR IGNORE to avoid duplicates
pub fn insert_or_get_spec(
    conn: &Connection,
    name: &str,
    base_url: &str,
    provider: &str,
) -> Result<i64> {
    // Try to insert (will be ignored if already exists)
    conn.execute(
        "INSERT OR IGNORE INTO specs (name, base_url, provider) VALUES (?1, ?2, ?3)",
        (name, base_url, provider),
    )?;

    // Get the ID (whether we just inserted it or it already existed)
    let id: i64 = conn.query_row("SELECT id FROM specs WHERE name = ?1", [name], |row| {
        row.get(0)
    })?;

    Ok(id)
}

/// Insert a snapshot, returning its ID
pub fn insert_snapshot(
    conn: &Connection,
    spec_id: i64,
    sha: &str,
    commit_date: &str,
) -> Result<i64> {
    // Get current timestamp for indexed_at
    let indexed_at = chrono::Utc::now().to_rfc3339();

    // Insert the snapshot
    conn.execute(
        "INSERT INTO snapshots (spec_id, sha, commit_date, indexed_at, is_latest)
         VALUES (?1, ?2, ?3, ?4, 0)",
        (spec_id, sha, commit_date, &indexed_at),
    )?;

    // Get the ID
    let id = conn.last_insert_rowid();

    Ok(id)
}

/// Bulk insert sections for a snapshot
pub fn insert_sections_bulk(
    conn: &Connection,
    snapshot_id: i64,
    sections: &[ParsedSection],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    {
        let mut stmt = tx.prepare(
            "INSERT INTO sections
             (snapshot_id, anchor, title, content_text, section_type, parent_anchor, prev_anchor, next_anchor, depth)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;

        for section in sections {
            stmt.execute((
                snapshot_id,
                &section.anchor,
                &section.title,
                &section.content_text,
                section.section_type.as_str(),
                &section.parent_anchor,
                &section.prev_anchor,
                &section.next_anchor,
                section.depth,
            ))?;
        }
    }

    tx.commit()?;
    Ok(())
}

/// Bulk insert references for a snapshot
pub fn insert_refs_bulk(
    conn: &Connection,
    snapshot_id: i64,
    refs: &[ParsedReference],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    {
        let mut stmt = tx.prepare(
            "INSERT INTO refs (snapshot_id, from_anchor, to_spec, to_anchor)
             VALUES (?1, ?2, ?3, ?4)",
        )?;

        for reference in refs {
            stmt.execute((
                snapshot_id,
                &reference.from_anchor,
                &reference.to_spec,
                &reference.to_anchor,
            ))?;
        }
    }

    tx.commit()?;
    Ok(())
}

/// Set a snapshot as the latest for its spec
/// Updates is_latest flag: sets all other snapshots for this spec to 0, then sets this one to 1
pub fn set_latest_snapshot(conn: &Connection, spec_id: i64, snapshot_id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    // Set all snapshots for this spec to not latest
    tx.execute(
        "UPDATE snapshots SET is_latest = 0 WHERE spec_id = ?1",
        [spec_id],
    )?;

    // Set this snapshot to latest
    tx.execute(
        "UPDATE snapshots SET is_latest = 1 WHERE id = ?1",
        [snapshot_id],
    )?;

    tx.commit()?;
    Ok(())
}

/// Record that we checked for updates for a spec
pub fn record_update_check(conn: &Connection, spec_id: i64) -> Result<()> {
    let timestamp = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT OR REPLACE INTO update_checks (spec_id, last_checked) VALUES (?1, ?2)",
        (spec_id, timestamp),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model::SectionType;

    #[test]
    fn test_insert_or_get_spec() {
        let conn = db::open_test_db().unwrap();

        // First insert
        let id1 =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();
        assert!(id1 > 0);

        // Second insert should return same ID
        let id2 =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();
        assert_eq!(id1, id2);

        // Different spec should get different ID
        let id3 =
            insert_or_get_spec(&conn, "DOM", "https://dom.spec.whatwg.org", "whatwg").unwrap();
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_insert_snapshot() {
        let conn = db::open_test_db().unwrap();

        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        let snapshot_id =
            insert_snapshot(&conn, spec_id, "abc123", "2026-01-01T00:00:00Z").unwrap();

        assert!(snapshot_id > 0);

        // Verify it was inserted
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM snapshots WHERE id = ?1",
                [snapshot_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_sections_bulk() {
        let conn = db::open_test_db().unwrap();

        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();
        let snapshot_id =
            insert_snapshot(&conn, spec_id, "abc123", "2026-01-01T00:00:00Z").unwrap();

        let sections = vec![
            ParsedSection {
                anchor: "intro".to_string(),
                title: Some("Introduction".to_string()),
                content_text: Some("This is the intro".to_string()),
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
            },
            ParsedSection {
                anchor: "details".to_string(),
                title: Some("Details".to_string()),
                content_text: Some("More details here".to_string()),
                section_type: SectionType::Heading,
                parent_anchor: Some("intro".to_string()),
                prev_anchor: None,
                next_anchor: None,
                depth: Some(3),
            },
        ];

        insert_sections_bulk(&conn, snapshot_id, &sections).unwrap();

        // Verify sections were inserted
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sections WHERE snapshot_id = ?1",
                [snapshot_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Verify FTS index was updated
        let fts_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sections_fts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fts_count, 2);
    }

    #[test]
    fn test_insert_refs_bulk() {
        let conn = db::open_test_db().unwrap();

        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();
        let snapshot_id =
            insert_snapshot(&conn, spec_id, "abc123", "2026-01-01T00:00:00Z").unwrap();

        let refs = vec![
            ParsedReference {
                from_anchor: "intro".to_string(),
                to_spec: "DOM".to_string(),
                to_anchor: "concept-tree".to_string(),
            },
            ParsedReference {
                from_anchor: "intro".to_string(),
                to_spec: "HTML".to_string(),
                to_anchor: "details".to_string(),
            },
        ];

        insert_refs_bulk(&conn, snapshot_id, &refs).unwrap();

        // Verify refs were inserted
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM refs WHERE snapshot_id = ?1",
                [snapshot_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_set_latest_snapshot() {
        let conn = db::open_test_db().unwrap();

        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        let snapshot1 = insert_snapshot(&conn, spec_id, "abc123", "2026-01-01T00:00:00Z").unwrap();
        let snapshot2 = insert_snapshot(&conn, spec_id, "def456", "2026-01-02T00:00:00Z").unwrap();

        // Set snapshot1 as latest
        set_latest_snapshot(&conn, spec_id, snapshot1).unwrap();

        let is_latest: i64 = conn
            .query_row(
                "SELECT is_latest FROM snapshots WHERE id = ?1",
                [snapshot1],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(is_latest, 1);

        // Set snapshot2 as latest
        set_latest_snapshot(&conn, spec_id, snapshot2).unwrap();

        let is_latest1: i64 = conn
            .query_row(
                "SELECT is_latest FROM snapshots WHERE id = ?1",
                [snapshot1],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(is_latest1, 0);

        let is_latest2: i64 = conn
            .query_row(
                "SELECT is_latest FROM snapshots WHERE id = ?1",
                [snapshot2],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(is_latest2, 1);
    }

    #[test]
    fn test_record_update_check() {
        let conn = db::open_test_db().unwrap();

        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        record_update_check(&conn, spec_id).unwrap();

        // Verify it was recorded
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM update_checks WHERE spec_id = ?1",
                [spec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Record again (should update, not insert new row)
        record_update_check(&conn, spec_id).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM update_checks WHERE spec_id = ?1",
                [spec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
