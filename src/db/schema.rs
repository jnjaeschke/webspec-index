use anyhow::Result;
use rusqlite::Connection;

pub fn initialize_schema(conn: &Connection) -> Result<()> {
    // Check if already initialized
    let table_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='specs'",
        [],
        |row| row.get(0),
    )?;

    if table_count > 0 {
        return Ok(());
    }

    conn.execute_batch(
        r#"
        CREATE TABLE specs (
            id          INTEGER PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE,
            base_url    TEXT NOT NULL,
            provider    TEXT NOT NULL
        );

        CREATE TABLE snapshots (
            id          INTEGER PRIMARY KEY,
            spec_id     INTEGER NOT NULL REFERENCES specs(id),
            sha         TEXT NOT NULL,
            commit_date TEXT NOT NULL,
            indexed_at  TEXT NOT NULL,
            is_latest   INTEGER NOT NULL DEFAULT 0,
            UNIQUE(spec_id, sha)
        );

        CREATE TABLE sections (
            id            INTEGER PRIMARY KEY,
            snapshot_id   INTEGER NOT NULL REFERENCES snapshots(id),
            anchor        TEXT NOT NULL,
            title         TEXT,
            content_text  TEXT,
            section_type  TEXT NOT NULL,
            parent_anchor TEXT,
            prev_anchor   TEXT,
            next_anchor   TEXT,
            depth         INTEGER,
            UNIQUE(snapshot_id, anchor)
        );

        CREATE INDEX idx_sections_parent ON sections(snapshot_id, parent_anchor);

        CREATE TABLE refs (
            id           INTEGER PRIMARY KEY,
            snapshot_id  INTEGER NOT NULL REFERENCES snapshots(id),
            from_anchor  TEXT NOT NULL,
            to_spec      TEXT NOT NULL,
            to_anchor    TEXT NOT NULL
        );

        CREATE INDEX idx_refs_outgoing ON refs(snapshot_id, from_anchor);
        CREATE INDEX idx_refs_incoming ON refs(snapshot_id, to_spec, to_anchor);

        CREATE TABLE update_checks (
            spec_id     INTEGER PRIMARY KEY REFERENCES specs(id),
            last_checked TEXT NOT NULL
        );

        CREATE VIRTUAL TABLE sections_fts USING fts5(
            anchor,
            title,
            content_text,
            content=sections,
            content_rowid=id
        );

        CREATE TRIGGER sections_ai AFTER INSERT ON sections BEGIN
            INSERT INTO sections_fts(rowid, anchor, title, content_text)
            VALUES (new.id, new.anchor, new.title, new.content_text);
        END;

        CREATE TRIGGER sections_ad AFTER DELETE ON sections BEGIN
            INSERT INTO sections_fts(sections_fts, rowid, anchor, title, content_text)
            VALUES ('delete', old.id, old.anchor, old.title, old.content_text);
        END;

        CREATE TRIGGER sections_au AFTER UPDATE ON sections BEGIN
            INSERT INTO sections_fts(sections_fts, rowid, anchor, title, content_text)
            VALUES ('delete', old.id, old.anchor, old.title, old.content_text);
            INSERT INTO sections_fts(rowid, anchor, title, content_text)
            VALUES (new.id, new.anchor, new.title, new.content_text);
        END;
        "#,
    )?;

    Ok(())
}

/// Run schema migrations for tables added after initial release.
/// Uses CREATE TABLE IF NOT EXISTS to be safe on both new and existing databases.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS repo_version_cache (
            repo        TEXT PRIMARY KEY,
            sha         TEXT NOT NULL,
            commit_date TEXT NOT NULL,
            checked_at  TEXT NOT NULL
        );",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_initialization() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"specs".to_string()));
        assert!(tables.contains(&"snapshots".to_string()));
        assert!(tables.contains(&"sections".to_string()));
        assert!(tables.contains(&"refs".to_string()));
        assert!(tables.contains(&"update_checks".to_string()));
    }

    #[test]
    fn test_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        // Should not fail on second call
        initialize_schema(&conn).unwrap();
    }

    #[test]
    fn test_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"repo_version_cache".to_string()));
    }

    #[test]
    fn test_migrations_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();
        run_migrations(&conn).unwrap();
        // Should not fail on second call
        run_migrations(&conn).unwrap();
    }
}
