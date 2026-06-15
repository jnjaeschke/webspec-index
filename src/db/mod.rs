pub mod queries;
pub mod schema;
pub mod write;

use anyhow::Result;
use rusqlite::Connection;
use std::path::PathBuf;

/// Get the database file path
/// Tests can override this by setting a different path
pub fn get_db_path() -> PathBuf {
    if let Ok(test_db) = std::env::var("SPEC_INDEX_TEST_DB") {
        PathBuf::from(test_db)
    } else {
        // Cross-platform: HOME on Unix, USERPROFILE/known-folder on Windows.
        let home = dirs::home_dir()
            .expect("could not determine home directory (set SPEC_INDEX_TEST_DB to override)");
        home.join(".webspec-index").join("index.db")
    }
}

/// Open or create the database, applying schema if needed.
///
/// On first creation (or after `clear-db`), seeds the spec list so that
/// `webspec-index specs` returns all known specs immediately.
pub fn open_or_create_db() -> Result<Connection> {
    let db_path = get_db_path();

    // Create parent directory if it doesn't exist
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(&db_path)?;
    schema::initialize_schema(&conn)?;
    schema::run_migrations(&conn)?;

    // Seed known specs (W3C, WHATWG, TC39, WebGPU). This is an upsert,
    // so it's safe to call on every open — new specs get added, existing
    // ones are left untouched.
    let _ = crate::spec_list::fetch_and_seed(&conn);

    Ok(conn)
}

#[cfg(test)]
pub fn open_test_db() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    schema::initialize_schema(&conn)?;
    schema::run_migrations(&conn)?;
    Ok(conn)
}
