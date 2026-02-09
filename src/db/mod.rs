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
        let home = std::env::var("HOME").expect("HOME environment variable not set");
        PathBuf::from(home).join(".webspec-index").join("index.db")
    }
}

/// Open or create the database, applying schema if needed
pub fn open_or_create_db() -> Result<Connection> {
    let db_path = get_db_path();

    // Create parent directory if it doesn't exist
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(&db_path)?;
    schema::initialize_schema(&conn)?;

    Ok(conn)
}

#[cfg(test)]
pub fn open_test_db() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    schema::initialize_schema(&conn)?;
    Ok(conn)
}
