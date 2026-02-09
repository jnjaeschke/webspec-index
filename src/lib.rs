//! WebSpec-Index: Query WHATWG/W3C web specifications
//!
//! This library provides parsing, indexing, and querying of web specifications.
//! It's designed to be used via Python bindings (PyO3), but can also be used directly from Rust.

pub mod db;
pub mod fetch;
pub mod format;
pub mod model;
pub mod parse;
pub mod provider;
pub mod spec_registry;

// Python bindings (only compiled when building as Python extension)
#[cfg(feature = "extension-module")]
pub mod python;

use anyhow::Result;

/// Parse a spec#anchor string into (spec, anchor) tuple
pub fn parse_spec_anchor(input: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = input.split('#').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid format. Expected SPEC#anchor (e.g., HTML#navigate)");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// Query a specific section in a specification
///
/// Returns complete section information including navigation, children, and cross-references.
///
/// # Arguments
/// * `spec_anchor` - Format: "SPEC#anchor" (e.g., "HTML#navigate")
/// * `sha` - Optional commit SHA for specific version
///
/// # Returns
/// `QueryResult` with section details, navigation, and references
pub async fn query_section(spec_anchor: &str, sha: Option<&str>) -> Result<model::QueryResult> {
    let (spec_name, anchor) = parse_spec_anchor(spec_anchor)?;
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    // Get spec info
    let spec = registry
        .find_spec(&spec_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;

    // Get snapshot and SHA
    let (snapshot_id, snapshot_sha) = if let Some(sha_str) = sha {
        let id = db::queries::get_snapshot_by_sha(&conn, &spec_name, sha_str)?
            .ok_or_else(|| anyhow::anyhow!("Snapshot not found for SHA: {}", sha_str))?;
        (id, sha_str.to_string())
    } else {
        // Ensure latest indexed
        let provider = registry.get_provider(spec)?;
        let id = fetch::ensure_latest_indexed(&conn, spec, provider).await?;
        // Get the SHA for this snapshot
        let sha_from_db: String = conn.query_row(
            "SELECT sha FROM snapshots WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        (id, sha_from_db)
    };

    // Get section
    let section = db::queries::get_section(&conn, snapshot_id, &anchor)?
        .ok_or_else(|| anyhow::anyhow!("Section not found: {}#{}", spec_name, anchor))?;

    // Get children
    let children = db::queries::get_children(&conn, snapshot_id, &anchor)?
        .iter()
        .map(|(child_anchor, title)| model::NavEntry {
            anchor: child_anchor.clone(),
            title: title.clone(),
        })
        .collect();

    // Get navigation (parent, prev, next)
    let navigation = model::Navigation {
        parent: section.parent_anchor.as_ref().and_then(|p| {
            db::queries::get_section(&conn, snapshot_id, p)
                .ok()?
                .map(|s| model::NavEntry {
                    anchor: s.anchor,
                    title: s.title,
                })
        }),
        prev: section.prev_anchor.as_ref().and_then(|p| {
            db::queries::get_section(&conn, snapshot_id, p)
                .ok()?
                .map(|s| model::NavEntry {
                    anchor: s.anchor,
                    title: s.title,
                })
        }),
        next: section.next_anchor.as_ref().and_then(|n| {
            db::queries::get_section(&conn, snapshot_id, n)
                .ok()?
                .map(|s| model::NavEntry {
                    anchor: s.anchor,
                    title: s.title,
                })
        }),
        children,
    };

    // Get outgoing references
    let out_refs = db::queries::get_outgoing_refs(&conn, snapshot_id, &anchor)?;
    let outgoing = out_refs
        .iter()
        .map(|(to_spec, to_anchor)| model::RefEntry {
            spec: to_spec.clone(),
            anchor: to_anchor.clone(),
        })
        .collect();

    // Get incoming references (from_spec, from_anchor)
    let in_refs = db::queries::get_incoming_refs(&conn, snapshot_id, &spec_name, &anchor)?;
    let incoming = in_refs
        .iter()
        .map(|(from_spec, from_anchor)| model::RefEntry {
            spec: from_spec.clone(),
            anchor: from_anchor.clone(),
        })
        .collect();

    Ok(model::QueryResult {
        spec: spec_name,
        sha: snapshot_sha,
        anchor: section.anchor,
        title: section.title,
        section_type: section.section_type.as_str().to_string(),
        content: section.content_text,
        navigation,
        outgoing_refs: outgoing,
        incoming_refs: incoming,
    })
}

/// Check if a section exists in the specification
///
/// # Arguments
/// * `spec_anchor` - Format: "SPEC#anchor"
///
/// # Returns
/// `ExistsResult` with existence status and section type if found
pub async fn check_exists(spec_anchor: &str) -> Result<model::ExistsResult> {
    let (spec_name, anchor) = parse_spec_anchor(spec_anchor)?;
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    // Get spec info
    let spec = registry
        .find_spec(&spec_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;

    // Ensure latest indexed
    let provider = registry.get_provider(spec)?;
    let snapshot_id = fetch::ensure_latest_indexed(&conn, spec, provider).await?;

    // Check if section exists
    let section = db::queries::get_section(&conn, snapshot_id, &anchor)?;
    let exists = section.is_some();
    let section_type = section
        .as_ref()
        .map(|s| s.section_type.as_str().to_string());

    Ok(model::ExistsResult {
        exists,
        spec: spec_name,
        anchor,
        section_type,
    })
}

/// Find anchors matching a glob pattern
///
/// # Arguments
/// * `pattern` - Glob pattern (e.g., "*-tree", "concept-*")
/// * `spec` - Optional spec name to limit search
/// * `limit` - Maximum number of results
///
/// # Returns
/// `AnchorsResult` with matching anchors
pub fn find_anchors(pattern: &str, spec: Option<&str>, limit: usize) -> Result<model::AnchorsResult> {
    let conn = db::open_or_create_db()?;

    // Convert glob pattern to SQL LIKE pattern
    let sql_pattern = pattern.replace('*', "%");

    // Find matching anchors
    let sql = if spec.is_some() {
        "SELECT s.anchor, sp.name, s.title, s.section_type FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE s.anchor LIKE ?1 AND sp.name = ?2 AND sn.is_latest = 1
         LIMIT ?3"
    } else {
        "SELECT s.anchor, sp.name, s.title, s.section_type FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE s.anchor LIKE ?1 AND sn.is_latest = 1
         LIMIT ?2"
    };

    let mut stmt = conn.prepare(sql)?;
    let results: Vec<(String, String, Option<String>, String)> = if let Some(spec_name) = spec {
        stmt.query_map((&sql_pattern, spec_name, limit), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map((&sql_pattern, limit), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    };

    // Convert to AnchorEntry format
    let entries: Vec<model::AnchorEntry> = results
        .iter()
        .map(|(anchor, spec_name, title, section_type)| model::AnchorEntry {
            spec: spec_name.clone(),
            anchor: anchor.clone(),
            title: title.clone(),
            section_type: section_type.clone(),
        })
        .collect();

    Ok(model::AnchorsResult {
        pattern: pattern.to_string(),
        results: entries,
    })
}

/// Full-text search across specifications
///
/// # Arguments
/// * `query` - Search query string
/// * `spec` - Optional spec name to limit search
/// * `limit` - Maximum number of results
///
/// # Returns
/// `SearchResult` with matching sections and snippets
pub fn search_sections(query: &str, spec: Option<&str>, limit: usize) -> Result<model::SearchResult> {
    let conn = db::open_or_create_db()?;

    // Search sections using FTS5
    let sql = if spec.is_some() {
        "SELECT s.anchor, sp.name, s.title, s.section_type, snippet(sections_fts, 2, '<mark>', '</mark>', '...', 64)
         FROM sections_fts
         JOIN sections s ON sections_fts.rowid = s.id
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE sections_fts MATCH ?1 AND sp.name = ?2 AND sn.is_latest = 1
         LIMIT ?3"
    } else {
        "SELECT s.anchor, sp.name, s.title, s.section_type, snippet(sections_fts, 2, '<mark>', '</mark>', '...', 64)
         FROM sections_fts
         JOIN sections s ON sections_fts.rowid = s.id
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE sections_fts MATCH ?1 AND sn.is_latest = 1
         LIMIT ?2"
    };

    let mut stmt = conn.prepare(sql)?;
    let results: Vec<(String, String, Option<String>, String, Option<String>)> =
        if let Some(spec_name) = spec {
            stmt.query_map((query, spec_name, limit), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map((query, limit), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

    // Convert to SearchEntry format
    let entries: Vec<model::SearchEntry> = results
        .iter()
        .map(|(anchor, spec_name, title, section_type, snippet)| model::SearchEntry {
            spec: spec_name.clone(),
            anchor: anchor.clone(),
            title: title.clone(),
            section_type: section_type.clone(),
            snippet: snippet.clone().unwrap_or_default(),
        })
        .collect();

    Ok(model::SearchResult {
        query: query.to_string(),
        results: entries,
    })
}

/// List all headings in a specification
///
/// # Arguments
/// * `spec` - Spec name
/// * `sha` - Optional commit SHA for specific version
///
/// # Returns
/// Vector of `ListEntry` with heading hierarchy
pub async fn list_headings(spec: &str, sha: Option<&str>) -> Result<Vec<model::ListEntry>> {
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    // Get spec info
    let spec_info = registry
        .find_spec(spec)
        .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec))?;

    // Get snapshot
    let snapshot_id = if let Some(sha_str) = sha {
        db::queries::get_snapshot_by_sha(&conn, spec, sha_str)?
            .ok_or_else(|| anyhow::anyhow!("Snapshot not found for SHA: {}", sha_str))?
    } else {
        // Ensure latest indexed
        let provider = registry.get_provider(spec_info)?;
        fetch::ensure_latest_indexed(&conn, spec_info, provider).await?
    };

    // Get all headings
    let headings = db::queries::list_headings(&conn, snapshot_id)?;

    // Convert to ListEntry format
    let entries: Vec<model::ListEntry> = headings
        .iter()
        .map(|h| model::ListEntry {
            anchor: h.anchor.clone(),
            title: h.title.clone(),
            depth: h.depth.unwrap_or(0),
            parent: h.parent_anchor.clone(),
        })
        .collect();

    Ok(entries)
}

/// Get cross-references for a section
///
/// # Arguments
/// * `spec_anchor` - Format: "SPEC#anchor"
/// * `direction` - "incoming", "outgoing", or "both"
/// * `sha` - Optional commit SHA for specific version
///
/// # Returns
/// `RefsResult` with incoming and/or outgoing references
pub async fn get_references(
    spec_anchor: &str,
    direction: &str,
    sha: Option<&str>,
) -> Result<model::RefsResult> {
    let (spec_name, anchor) = parse_spec_anchor(spec_anchor)?;
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    // Get spec info
    let spec = registry
        .find_spec(&spec_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;

    // Get snapshot
    let snapshot_id = if let Some(sha_str) = sha {
        db::queries::get_snapshot_by_sha(&conn, &spec_name, sha_str)?
            .ok_or_else(|| anyhow::anyhow!("Snapshot not found for SHA: {}", sha_str))?
    } else {
        // Ensure latest indexed
        let provider = registry.get_provider(spec)?;
        fetch::ensure_latest_indexed(&conn, spec, provider).await?
    };

    // Get references based on direction
    let outgoing = if direction == "outgoing" || direction == "both" {
        let out_refs = db::queries::get_outgoing_refs(&conn, snapshot_id, &anchor)?;
        Some(
            out_refs
                .iter()
                .map(|(to_spec, to_anchor)| model::RefEntry {
                    spec: to_spec.clone(),
                    anchor: to_anchor.clone(),
                })
                .collect(),
        )
    } else {
        None
    };

    let incoming = if direction == "incoming" || direction == "both" {
        let in_refs = db::queries::get_incoming_refs(&conn, snapshot_id, &spec_name, &anchor)?;
        Some(
            in_refs
                .iter()
                .map(|(from_spec, from_anchor)| model::RefEntry {
                    spec: from_spec.clone(),
                    anchor: from_anchor.clone(),
                })
                .collect(),
        )
    } else {
        None
    };

    Ok(model::RefsResult {
        anchor,
        direction: direction.to_string(),
        outgoing,
        incoming,
    })
}

/// Update specifications to latest versions
///
/// # Arguments
/// * `spec` - Optional spec name (updates all if None)
/// * `force` - Force update even if recently checked
///
/// # Returns
/// Vector of tuples (spec_name, Option<snapshot_id>)
/// - None indicates spec was already up to date
pub async fn update_specs(spec: Option<&str>, force: bool) -> Result<Vec<(String, Option<i64>)>> {
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    let mut results = Vec::new();

    if let Some(spec_name) = spec {
        // Update single spec
        let spec_info = registry
            .find_spec(spec_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;
        let provider = registry.get_provider(spec_info)?;

        let snapshot_id = fetch::update_if_needed(&conn, spec_info, provider, force).await?;
        results.push((spec_name.to_string(), snapshot_id));
    } else {
        // Update all specs
        let all_results = fetch::update_all_specs(&conn, &registry, force).await;

        for (spec_name, result) in all_results {
            match result {
                Ok(snapshot_id) => results.push((spec_name, snapshot_id)),
                Err(e) => {
                    eprintln!("Failed to update {}: {}", spec_name, e);
                    results.push((spec_name, None));
                }
            }
        }
    }

    Ok(results)
}

/// Clear the database (remove all indexed data)
///
/// # Returns
/// Path to the deleted database file
pub fn clear_database() -> Result<String> {
    let db_path = db::get_db_path();

    if !db_path.exists() {
        anyhow::bail!("Database does not exist: {}", db_path.display());
    }

    std::fs::remove_file(&db_path)?;
    Ok(db_path.display().to_string())
}
