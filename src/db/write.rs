// Write operations on the database
use crate::model::{ParsedIdlDefinition, ParsedReference, ParsedSection};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

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

/// Upsert spec metadata from the authoritative spec list.
///
/// Updates base_url and provider if they changed, and clears any stale indexed
/// data so the spec gets re-fetched from the correct URL.
pub fn seed_spec(conn: &Connection, name: &str, base_url: &str, provider: &str) -> Result<()> {
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, base_url FROM specs WHERE name = ?1",
            [name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    match existing {
        None => {
            conn.execute(
                "INSERT INTO specs (name, base_url, provider) VALUES (?1, ?2, ?3)",
                (name, base_url, provider),
            )?;
        }
        Some((id, old_url)) if old_url != base_url => {
            conn.execute(
                "UPDATE specs SET base_url = ?1, provider = ?2 WHERE id = ?3",
                (base_url, provider, id),
            )?;
            delete_spec_data(conn, id)?;
        }
        _ => {}
    }
    Ok(())
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
        "INSERT INTO snapshots (spec_id, sha, commit_date, indexed_at)
         VALUES (?1, ?2, ?3, ?4)",
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

/// Bulk insert IDL definitions for a snapshot
pub fn insert_idl_defs_bulk(
    conn: &Connection,
    snapshot_id: i64,
    defs: &[ParsedIdlDefinition],
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    {
        let mut stmt = tx.prepare(
            "INSERT INTO idl_defs (snapshot_id, anchor, name, owner, kind, canonical_name, idl_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;

        for def in defs {
            stmt.execute((
                snapshot_id,
                &def.anchor,
                &def.name,
                &def.owner,
                &def.kind,
                &def.canonical_name,
                &def.idl_text,
            ))?;
        }
    }

    tx.commit()?;
    Ok(())
}

/// Insert a PR snapshot, returning its ID.
/// Sets pr_number and merge_base_sha in addition to the standard snapshot fields.
pub fn insert_pr_snapshot(
    conn: &Connection,
    spec_id: i64,
    sha: &str,
    commit_date: &str,
    pr_number: i64,
    merge_base_sha: &str,
) -> Result<i64> {
    let indexed_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO snapshots (spec_id, sha, commit_date, indexed_at, pr_number, merge_base_sha)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (spec_id, sha, commit_date, &indexed_at, pr_number, merge_base_sha),
    )?;
    Ok(conn.last_insert_rowid())
}

/// Delete all indexed data for a specific PR number.
pub fn delete_pr_data(conn: &Connection, spec_id: i64, pr_number: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM refs WHERE snapshot_id IN \
         (SELECT id FROM snapshots WHERE spec_id = ?1 AND pr_number = ?2)",
        (spec_id, pr_number),
    )?;
    tx.execute(
        "DELETE FROM idl_defs WHERE snapshot_id IN \
         (SELECT id FROM snapshots WHERE spec_id = ?1 AND pr_number = ?2)",
        (spec_id, pr_number),
    )?;
    tx.execute(
        "DELETE FROM sections WHERE snapshot_id IN \
         (SELECT id FROM snapshots WHERE spec_id = ?1 AND pr_number = ?2)",
        (spec_id, pr_number),
    )?;
    tx.execute(
        "DELETE FROM snapshots WHERE spec_id = ?1 AND pr_number = ?2",
        (spec_id, pr_number),
    )?;
    tx.commit()?;
    Ok(())
}

/// Delete trunk indexed data for a spec (snapshot, sections, refs).
/// Only deletes snapshots with `sha LIKE 'hash:%'` and `pr_number IS NULL`,
/// preserving PR snapshots and commit snapshots (merge bases).
/// Used before re-indexing to avoid clobbering PR data.
pub fn delete_spec_data(conn: &Connection, spec_id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "DELETE FROM refs WHERE snapshot_id IN \
         (SELECT id FROM snapshots WHERE spec_id = ?1 AND pr_number IS NULL AND sha LIKE 'hash:%')",
        [spec_id],
    )?;
    tx.execute(
        "DELETE FROM idl_defs WHERE snapshot_id IN \
         (SELECT id FROM snapshots WHERE spec_id = ?1 AND pr_number IS NULL AND sha LIKE 'hash:%')",
        [spec_id],
    )?;
    tx.execute(
        "DELETE FROM sections WHERE snapshot_id IN \
         (SELECT id FROM snapshots WHERE spec_id = ?1 AND pr_number IS NULL AND sha LIKE 'hash:%')",
        [spec_id],
    )?;
    tx.execute(
        "DELETE FROM snapshots WHERE spec_id = ?1 AND pr_number IS NULL AND sha LIKE 'hash:%'",
        [spec_id],
    )?;

    tx.commit()?;
    Ok(())
}

/// Record spec sync metadata for freshness/content-hash based updates.
pub fn record_update_check(
    conn: &Connection,
    spec_id: i64,
    last_checked: &str,
    last_indexed: Option<&str>,
    content_hash: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO update_checks (spec_id, last_checked, last_indexed, content_hash)
         VALUES (?1, ?2, ?3, ?4)",
        (spec_id, last_checked, last_indexed, content_hash),
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
    fn test_delete_spec_data() {
        let conn = db::open_test_db().unwrap();

        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();
        // Use hash: prefix — delete_spec_data only removes trunk (hash:) snapshots
        let snapshot_id =
            insert_snapshot(&conn, spec_id, "hash:abc123", "2026-01-01T00:00:00Z").unwrap();

        let sections = vec![crate::model::ParsedSection {
            anchor: "intro".to_string(),
            title: Some("Introduction".to_string()),
            content_text: None,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(2),
        }];
        insert_sections_bulk(&conn, snapshot_id, &sections).unwrap();

        // Verify data exists
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sections", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Delete spec data
        delete_spec_data(&conn, spec_id).unwrap();

        // Verify trunk snapshot is gone
        let snap_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(snap_count, 0);

        let sec_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sections", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sec_count, 0);
    }

    #[test]
    fn test_insert_pr_snapshot() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        let snapshot_id = insert_pr_snapshot(
            &conn, spec_id, "hash:abc123", "2026-01-01T00:00:00Z", 12345, "def456full",
        ).unwrap();
        assert!(snapshot_id > 0);

        let (pr, base): (Option<i64>, Option<String>) = conn.query_row(
            "SELECT pr_number, merge_base_sha FROM snapshots WHERE id = ?1",
            [snapshot_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(pr, Some(12345));
        assert_eq!(base.as_deref(), Some("def456full"));
    }

    #[test]
    fn test_delete_pr_data() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        // Insert trunk snapshot
        let trunk_id = insert_snapshot(&conn, spec_id, "trunk1", "2026-01-01T00:00:00Z").unwrap();
        insert_sections_bulk(&conn, trunk_id, &[ParsedSection {
            anchor: "intro".into(), title: Some("Intro".into()), content_text: None,
            section_type: SectionType::Heading, parent_anchor: None,
            prev_anchor: None, next_anchor: None, depth: Some(2),
        }]).unwrap();

        // Insert PR snapshot
        let pr_id = insert_pr_snapshot(&conn, spec_id, "pr1", "2026-01-01T00:00:00Z", 123, "base1").unwrap();
        insert_sections_bulk(&conn, pr_id, &[ParsedSection {
            anchor: "new-section".into(), title: Some("New".into()), content_text: None,
            section_type: SectionType::Heading, parent_anchor: None,
            prev_anchor: None, next_anchor: None, depth: Some(2),
        }]).unwrap();

        // Delete only PR data
        delete_pr_data(&conn, spec_id, 123).unwrap();

        // Trunk still exists
        let trunk_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM snapshots WHERE spec_id = ?1 AND pr_number IS NULL",
            [spec_id], |row| row.get(0),
        ).unwrap();
        assert_eq!(trunk_count, 1);

        // PR is gone
        let pr_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM snapshots WHERE spec_id = ?1 AND pr_number = 123",
            [spec_id], |row| row.get(0),
        ).unwrap();
        assert_eq!(pr_count, 0);
    }

    #[test]
    fn test_delete_spec_data_preserves_pr_and_commit_snapshots() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        // Insert trunk (hash: prefix), commit snapshot (real SHA), and PR snapshot
        insert_snapshot(&conn, spec_id, "hash:abc123", "2026-01-01T00:00:00Z").unwrap();
        insert_snapshot(&conn, spec_id, "74cbe0af38fee8a0", "2026-01-01T00:00:00Z").unwrap();
        insert_pr_snapshot(&conn, spec_id, "pr:123:def", "2026-01-01T00:00:00Z", 123, "74cbe0af38fee8a0").unwrap();

        // delete_spec_data should only delete trunk (hash: prefix)
        delete_spec_data(&conn, spec_id).unwrap();

        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM snapshots WHERE spec_id = ?1",
            [spec_id], |row| row.get(0),
        ).unwrap();
        assert_eq!(total, 2); // PR snapshot + commit snapshot remain
    }

    #[test]
    fn test_insert_idl_defs_bulk() {
        let conn = db::open_test_db().unwrap();
        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();
        let snapshot_id =
            insert_snapshot(&conn, spec_id, "abc123", "2026-01-01T00:00:00Z").unwrap();

        let defs = vec![
            ParsedIdlDefinition {
                anchor: "dom-window".to_string(),
                name: "Window".to_string(),
                owner: None,
                kind: "interface".to_string(),
                canonical_name: "Window".to_string(),
                idl_text: Some("interface Window {};".to_string()),
            },
            ParsedIdlDefinition {
                anchor: "dom-window-navigation".to_string(),
                name: "navigation".to_string(),
                owner: Some("Window".to_string()),
                kind: "attribute".to_string(),
                canonical_name: "Window.navigation".to_string(),
                idl_text: Some(
                    "interface Window { attribute Navigation navigation; };".to_string(),
                ),
            },
        ];

        insert_idl_defs_bulk(&conn, snapshot_id, &defs).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM idl_defs WHERE snapshot_id = ?1",
                [snapshot_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_record_update_check() {
        let conn = db::open_test_db().unwrap();

        let spec_id =
            insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg").unwrap();

        record_update_check(
            &conn,
            spec_id,
            "2026-01-01T00:00:00Z",
            Some("2026-01-01T00:00:00Z"),
            Some("deadbeef"),
        )
        .unwrap();

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
        record_update_check(
            &conn,
            spec_id,
            "2026-01-02T00:00:00Z",
            None,
            Some("beadfeed"),
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM update_checks WHERE spec_id = ?1",
                [spec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let (checked, indexed, hash): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT last_checked, last_indexed, content_hash FROM update_checks WHERE spec_id = ?1",
                [spec_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(checked, "2026-01-02T00:00:00Z");
        assert_eq!(indexed, None);
        assert_eq!(hash.as_deref(), Some("beadfeed"));
    }
}
