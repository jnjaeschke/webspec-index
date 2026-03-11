//! WebSpec-Index: Query WHATWG/W3C web specifications
//!
//! This library provides parsing, indexing, and querying of web specifications.
//! It's designed to be used via Python bindings (PyO3), but can also be used directly from Rust.

pub mod db;
pub mod fetch;
pub mod format;
pub mod lsp;
pub mod model;
pub mod parse;
pub mod spec_registry;

use anyhow::Result;
use regex::Regex;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet, VecDeque};

/// Parse a spec#anchor string or full URL into (spec, anchor) tuple
pub fn parse_spec_anchor(input: &str) -> Result<(String, String)> {
    let trimmed = input.trim();

    // Try URL first
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let registry = spec_registry::SpecRegistry::new();
        if let Some((spec, anchor)) = registry.resolve_url(trimmed) {
            return Ok((spec, anchor));
        }
        anyhow::bail!(
            "URL not recognized. Use a known SPEC#anchor, or a whitelisted URL domain with a #fragment: {trimmed}"
        );
    }

    // Accept host-style URLs without an explicit scheme, e.g. html.spec.whatwg.org/#navigate
    if trimmed.contains('#') && trimmed.contains('/') && !trimmed.contains("://") {
        let maybe_url = format!("https://{}", trimmed.trim_start_matches('/'));
        let registry = spec_registry::SpecRegistry::new();
        if let Some((spec, anchor)) = registry.resolve_url(&maybe_url) {
            return Ok((spec, anchor));
        }
    }

    // Fall back to SPEC#anchor
    let parts: Vec<&str> = trimmed.split('#').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid format. Expected SPEC#anchor or a full spec URL");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

/// Return indexed/discovered spec base URLs
pub fn spec_urls() -> Vec<model::SpecUrlEntry> {
    let conn = match db::open_or_create_db() {
        Ok(conn) => conn,
        Err(_) => return vec![],
    };

    db::queries::list_specs(&conn)
        .unwrap_or_default()
        .into_iter()
        .map(|(spec, base_url, _provider)| model::SpecUrlEntry { spec, base_url })
        .collect()
}

fn resolve_spec_metadata(
    conn: &Connection,
    registry: &spec_registry::SpecRegistry,
    spec_name: &str,
) -> Result<(String, String, String)> {
    if let Some((name, base_url, provider)) = db::queries::get_spec_meta(conn, spec_name)? {
        return Ok((name, base_url, provider));
    }

    if let Some(base_url) = spec_registry::auto_spec_base_url(spec_name) {
        let provider = spec_registry::provider_for_base_url(&base_url).to_string();
        let name = spec_name.to_string();
        return Ok((name, base_url, provider));
    }

    if let Some((base_url, provider)) = registry.infer_base_url_from_spec_name(spec_name) {
        // Canonicalize name from URL for stable refs.
        if let Some((canonical_name, _)) = registry.resolve_url(&format!("{base_url}#x")) {
            return Ok((canonical_name, base_url, provider));
        }
        return Ok((spec_name.to_string(), base_url, provider));
    }

    anyhow::bail!("Unknown spec: {}", spec_name)
}

async fn ensure_indexed_for_spec_name(
    conn: &Connection,
    registry: &spec_registry::SpecRegistry,
    spec_name: &str,
) -> Result<(i64, String)> {
    let (canonical_name, base_url, provider) = resolve_spec_metadata(conn, registry, spec_name)?;
    let snapshot_id = fetch::ensure_indexed(conn, &canonical_name, &base_url, &provider).await?;
    Ok((snapshot_id, canonical_name))
}

/// Query a specific section in a specification
///
/// Returns complete section information including navigation, children, and cross-references.
///
/// # Arguments
/// * `spec_anchor` - Format: "SPEC#anchor" (e.g., "HTML#navigate")
pub async fn query_section(spec_anchor: &str) -> Result<model::QueryResult> {
    let (spec_name, anchor) = parse_spec_anchor(spec_anchor)?;
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();
    let (snapshot_id, spec_name) =
        ensure_indexed_for_spec_name(&conn, &registry, &spec_name).await?;

    let snapshot_sha: String = conn.query_row(
        "SELECT sha FROM snapshots WHERE id = ?1",
        [snapshot_id],
        |row| row.get(0),
    )?;

    let section = db::queries::get_section(&conn, snapshot_id, &anchor)?
        .ok_or_else(|| anyhow::anyhow!("Section not found: {}#{}", spec_name, anchor))?;

    let children = db::queries::get_children(&conn, snapshot_id, &anchor)?
        .iter()
        .map(|(child_anchor, title)| model::NavEntry {
            anchor: child_anchor.clone(),
            title: title.clone(),
        })
        .collect();

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

    let out_refs = db::queries::get_outgoing_refs(&conn, snapshot_id, &anchor)?;
    let outgoing = out_refs
        .iter()
        .map(|(to_spec, to_anchor)| model::RefEntry {
            spec: to_spec.clone(),
            anchor: to_anchor.clone(),
        })
        .collect();

    let in_refs = db::queries::get_incoming_refs(&conn, &spec_name, &anchor)?;
    let incoming = in_refs
        .iter()
        .map(|(from_spec, from_anchor)| model::RefEntry {
            spec: from_spec.clone(),
            anchor: from_anchor.clone(),
        })
        .collect();

    Ok(model::QueryResult {
        spec: spec_name.clone(),
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
    let (snapshot_id, spec_name) =
        ensure_indexed_for_spec_name(&conn, &registry, &spec_name).await?;

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
pub fn find_anchors(
    pattern: &str,
    spec: Option<&str>,
    limit: usize,
) -> Result<model::AnchorsResult> {
    let conn = db::open_or_create_db()?;

    // Convert glob pattern to SQL LIKE pattern
    let sql_pattern = pattern.replace('*', "%");

    // Find matching anchors
    let sql = if spec.is_some() {
        "SELECT s.anchor, sp.name, s.title, s.section_type FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE s.anchor LIKE ?1 AND sp.name = ?2          LIMIT ?3"
    } else {
        "SELECT s.anchor, sp.name, s.title, s.section_type FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE s.anchor LIKE ?1          LIMIT ?2"
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
        .map(
            |(anchor, spec_name, title, section_type)| model::AnchorEntry {
                spec: spec_name.clone(),
                anchor: anchor.clone(),
                title: title.clone(),
                section_type: section_type.clone(),
            },
        )
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
pub fn search_sections(
    query: &str,
    spec: Option<&str>,
    limit: usize,
) -> Result<model::SearchResult> {
    let conn = db::open_or_create_db()?;
    let entries = match search_sections_fts(&conn, query, spec, limit) {
        Ok(entries) => entries,
        Err(err) if is_fts_syntax_error(&err) => {
            if let Some(sanitized) = sanitize_for_fts(query) {
                search_sections_fts(&conn, &sanitized, spec, limit)?
            } else {
                vec![]
            }
        }
        Err(err) => return Err(err.into()),
    };

    Ok(model::SearchResult {
        query: query.to_string(),
        results: entries,
    })
}

fn search_sections_fts(
    conn: &Connection,
    query: &str,
    spec: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<model::SearchEntry>> {
    let sql = if spec.is_some() {
        "SELECT s.anchor, sp.name, s.title, s.section_type, snippet(sections_fts, 2, '<mark>', '</mark>', '...', 64)
         FROM sections_fts
         JOIN sections s ON sections_fts.rowid = s.id
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE sections_fts MATCH ?1 AND sp.name = ?2          LIMIT ?3"
    } else {
        "SELECT s.anchor, sp.name, s.title, s.section_type, snippet(sections_fts, 2, '<mark>', '</mark>', '...', 64)
         FROM sections_fts
         JOIN sections s ON sections_fts.rowid = s.id
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE sections_fts MATCH ?1          LIMIT ?2"
    };

    let mut stmt = conn.prepare(sql)?;
    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<model::SearchEntry> {
        Ok(model::SearchEntry {
            anchor: row.get(0)?,
            spec: row.get(1)?,
            title: row.get(2)?,
            section_type: row.get(3)?,
            snippet: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    };
    if let Some(spec_name) = spec {
        stmt.query_map((query, spec_name, limit), map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
    } else {
        stmt.query_map((query, limit), map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
    }
}

fn is_fts_syntax_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(_, Some(message)) => message.contains("fts5: syntax error"),
        _ => false,
    }
}

fn sanitize_for_fts(query: &str) -> Option<String> {
    let terms = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// List all headings in a specification
///
/// # Arguments
/// * `spec` - Spec name
///
/// # Returns
/// Vector of `ListEntry` with heading hierarchy
pub async fn list_headings(spec: &str) -> Result<Vec<model::ListEntry>> {
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();
    let (snapshot_id, _spec_name) = ensure_indexed_for_spec_name(&conn, &registry, spec).await?;

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
///
/// # Returns
/// `RefsResult` with incoming and/or outgoing references
pub async fn get_references(spec_anchor: &str, direction: &str) -> Result<model::RefsResult> {
    let (spec_name, anchor) = parse_spec_anchor(spec_anchor)?;
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();
    let (snapshot_id, spec_name) =
        ensure_indexed_for_spec_name(&conn, &registry, &spec_name).await?;

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
        let in_refs = db::queries::get_incoming_refs(&conn, &spec_name, &anchor)?;
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefDirection {
    Incoming,
    Outgoing,
    Both,
}

fn parse_ref_direction(direction: &str) -> Result<RefDirection> {
    match direction.to_ascii_lowercase().as_str() {
        "incoming" => Ok(RefDirection::Incoming),
        "outgoing" => Ok(RefDirection::Outgoing),
        "both" => Ok(RefDirection::Both),
        _ => anyhow::bail!(
            "Invalid direction: {} (expected incoming|outgoing|both)",
            direction
        ),
    }
}

fn node_id(spec: &str, anchor: &str) -> String {
    format!("{spec}#{anchor}")
}

#[derive(Clone)]
struct GraphFilters {
    include: Vec<String>,
    exclude: Vec<String>,
    same_spec_only: bool,
}

fn compile_pattern(pattern: &str) -> Result<Regex> {
    if let Some(rest) = pattern.strip_prefix("re:") {
        return Regex::new(rest)
            .map_err(|e| anyhow::anyhow!("Invalid regex pattern '{}': {}", pattern, e));
    }

    let mut re = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            _ => re.push_str(&regex::escape(&ch.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).map_err(|e| anyhow::anyhow!("Invalid wildcard pattern '{}': {}", pattern, e))
}

struct CompiledGraphFilters {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
    same_spec_only: bool,
}

impl CompiledGraphFilters {
    fn from_filters(filters: &GraphFilters) -> Result<Self> {
        let include = filters
            .include
            .iter()
            .map(|p| compile_pattern(p))
            .collect::<Result<Vec<_>>>()?;
        let exclude = filters
            .exclude
            .iter()
            .map(|p| compile_pattern(p))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            include,
            exclude,
            same_spec_only: filters.same_spec_only,
        })
    }

    fn matches_node(&self, node_id: &str, root_id: &str) -> bool {
        if node_id == root_id {
            return true;
        }

        if !self.include.is_empty() && !self.include.iter().any(|re| re.is_match(node_id)) {
            return false;
        }

        if self.exclude.iter().any(|re| re.is_match(node_id)) {
            return false;
        }

        true
    }
}

fn section_meta(
    conn: &Connection,
    spec: &str,
    anchor: &str,
) -> Result<Option<(Option<String>, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT s.title, s.section_type FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE sp.name = ?1 AND s.anchor = ?2
         LIMIT 1",
    )?;

    let mut rows = stmt.query((spec, anchor))?;
    if let Some(row) = rows.next()? {
        let title: Option<String> = row.get(0)?;
        let section_type: Option<String> = row.get(1)?;
        Ok(Some((title, section_type)))
    } else {
        Ok(None)
    }
}

fn outgoing_refs_for_node(
    conn: &Connection,
    spec: &str,
    anchor: &str,
) -> Result<Vec<(String, String)>> {
    let Some(snapshot_id) = db::queries::get_snapshot(conn, spec)? else {
        return Ok(vec![]);
    };
    db::queries::get_outgoing_refs(conn, snapshot_id, anchor)
}

fn build_graph_from_conn(
    conn: &Connection,
    root_spec: &str,
    root_anchor: &str,
    direction: &str,
    max_depth: usize,
    max_nodes: usize,
    filters: &GraphFilters,
) -> Result<model::GraphResult> {
    if max_nodes == 0 {
        anyhow::bail!("max_nodes must be greater than 0");
    }

    let dir = parse_ref_direction(direction)?;
    let compiled_filters = CompiledGraphFilters::from_filters(filters)?;

    let mut visited: HashSet<(String, String)> = HashSet::new();
    let mut queue: VecDeque<(String, String, usize)> = VecDeque::new();
    let mut edges: HashSet<(String, String)> = HashSet::new();
    let mut truncated = false;

    visited.insert((root_spec.to_string(), root_anchor.to_string()));
    queue.push_back((root_spec.to_string(), root_anchor.to_string(), 0));

    while let Some((spec, anchor, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }

        if dir == RefDirection::Outgoing || dir == RefDirection::Both {
            for (to_spec, to_anchor) in outgoing_refs_for_node(conn, &spec, &anchor)? {
                if compiled_filters.same_spec_only && (to_spec != root_spec || spec != root_spec) {
                    continue;
                }
                let from_id = node_id(&spec, &anchor);
                let to_id = node_id(&to_spec, &to_anchor);
                if from_id == to_id {
                    continue;
                }
                edges.insert((from_id, to_id));

                if visited.insert((to_spec.clone(), to_anchor.clone())) {
                    if visited.len() > max_nodes {
                        visited.remove(&(to_spec, to_anchor));
                        truncated = true;
                    } else {
                        queue.push_back((to_spec, to_anchor, depth + 1));
                    }
                }
            }
        }

        if dir == RefDirection::Incoming || dir == RefDirection::Both {
            for (from_spec, from_anchor) in db::queries::get_incoming_refs(conn, &spec, &anchor)? {
                if compiled_filters.same_spec_only && (from_spec != root_spec || spec != root_spec)
                {
                    continue;
                }
                let from_id = node_id(&from_spec, &from_anchor);
                let to_id = node_id(&spec, &anchor);
                if from_id == to_id {
                    continue;
                }
                edges.insert((from_id, to_id));

                if visited.insert((from_spec.clone(), from_anchor.clone())) {
                    if visited.len() > max_nodes {
                        visited.remove(&(from_spec, from_anchor));
                        truncated = true;
                    } else {
                        queue.push_back((from_spec, from_anchor, depth + 1));
                    }
                }
            }
        }
    }

    let mut nodes: Vec<model::GraphNode> = visited
        .into_iter()
        .map(|(spec, anchor)| {
            let id = node_id(&spec, &anchor);
            let (title, section_type) = section_meta(conn, &spec, &anchor)?.unwrap_or((None, None));
            Ok(model::GraphNode {
                id,
                spec,
                anchor,
                title,
                section_type,
                filter_role: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut edge_list: Vec<model::GraphEdge> = edges
        .into_iter()
        .map(|(from, to)| model::GraphEdge {
            from,
            to,
            kind: "reference".to_string(),
        })
        .collect();

    let root_id = node_id(root_spec, root_anchor);
    let filter_active =
        !compiled_filters.include.is_empty() || !compiled_filters.exclude.is_empty();
    let all_ids: HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();

    let mut matched_ids: HashSet<String> = if filter_active {
        nodes
            .iter()
            .filter_map(|n| {
                if compiled_filters.matches_node(&n.id, &root_id) {
                    Some(n.id.clone())
                } else {
                    None
                }
            })
            .collect()
    } else {
        all_ids.clone()
    };
    matched_ids.insert(root_id.clone());

    // Build undirected adjacency from all currently known edges.
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &edge_list {
        adjacency
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
        adjacency
            .entry(edge.to.clone())
            .or_default()
            .push(edge.from.clone());
    }

    // BFS tree for shortest paths from root (undirected).
    let mut parent: HashMap<String, String> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut bfs: VecDeque<String> = VecDeque::new();
    seen.insert(root_id.clone());
    bfs.push_back(root_id.clone());
    while let Some(current) = bfs.pop_front() {
        if let Some(neighbors) = adjacency.get(&current) {
            for neighbor in neighbors {
                if seen.insert(neighbor.clone()) {
                    parent.insert(neighbor.clone(), current.clone());
                    bfs.push_back(neighbor.clone());
                }
            }
        }
    }

    // Keep matched nodes and include bridge nodes on shortest root paths.
    let mut kept_ids: HashSet<String> = HashSet::new();
    kept_ids.insert(root_id.clone());
    for id in &matched_ids {
        if !seen.contains(id) {
            continue;
        }
        let mut cur = id.clone();
        kept_ids.insert(cur.clone());
        while let Some(p) = parent.get(&cur) {
            kept_ids.insert(p.clone());
            if *p == root_id {
                break;
            }
            cur = p.clone();
        }
    }

    nodes.retain(|n| kept_ids.contains(&n.id));
    edge_list.retain(|e| kept_ids.contains(&e.from) && kept_ids.contains(&e.to));

    // Final prune for accidental disconnected remnants in kept graph.
    let mut kept_adj: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &edge_list {
        kept_adj
            .entry(edge.from.clone())
            .or_default()
            .push(edge.to.clone());
        kept_adj
            .entry(edge.to.clone())
            .or_default()
            .push(edge.from.clone());
    }
    let mut connected: HashSet<String> = HashSet::new();
    let mut connected_q: VecDeque<String> = VecDeque::new();
    connected.insert(root_id.clone());
    connected_q.push_back(root_id.clone());
    while let Some(current) = connected_q.pop_front() {
        if let Some(neighbors) = kept_adj.get(&current) {
            for neighbor in neighbors {
                if connected.insert(neighbor.clone()) {
                    connected_q.push_back(neighbor.clone());
                }
            }
        }
    }

    nodes.retain(|n| connected.contains(&n.id));
    edge_list.retain(|e| connected.contains(&e.from) && connected.contains(&e.to));

    if filter_active {
        for node in &mut nodes {
            if node.id == root_id {
                node.filter_role = Some("root".to_string());
            } else if matched_ids.contains(&node.id) {
                node.filter_role = Some("matched".to_string());
            } else {
                node.filter_role = Some("bridge".to_string());
            }
        }
    }

    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    edge_list.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));

    Ok(model::GraphResult {
        root: model::GraphRoot {
            spec: root_spec.to_string(),
            anchor: root_anchor.to_string(),
        },
        direction: direction.to_ascii_lowercase(),
        max_depth,
        max_nodes,
        truncated,
        nodes,
        edges: edge_list,
    })
}

#[derive(Clone)]
struct Candidate {
    spec: String,
    anchor: String,
    title: Option<String>,
    section_type: String,
    score: i32,
}

fn resolve_find_references_candidates(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<Candidate>> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return Ok(vec![]);
    }

    let mut stmt = conn.prepare(
        "SELECT sp.name, s.anchor, s.title, s.section_type
         FROM sections s
         JOIN snapshots sn ON s.snapshot_id = sn.id
         JOIN specs sp ON sn.spec_id = sp.id
         WHERE LOWER(s.anchor) LIKE ?1 OR LOWER(COALESCE(s.title, '')) LIKE ?2
         LIMIT 1000",
    )?;

    let (anchor_like, title_like) = if let Some((_owner, member)) = q.split_once('.') {
        (format!("%{}%", member), format!("%{}%", member))
    } else {
        (format!("%{}%", q), format!("%{}%", q))
    };

    let mut rows = stmt.query((anchor_like, title_like))?;
    let mut candidates = Vec::new();

    if let Some((owner, member)) = q.split_once('.') {
        let owner = owner.trim();
        let member = member.trim();

        while let Some(row) = rows.next()? {
            let spec: String = row.get(0)?;
            let anchor: String = row.get(1)?;
            let title: Option<String> = row.get(2)?;
            let section_type: String = row.get(3)?;
            let anchor_l = anchor.to_ascii_lowercase();
            let title_l = title.as_deref().unwrap_or("").to_ascii_lowercase();

            let mut score = 0;
            if anchor_l.contains(member) {
                score += 40;
            }
            if anchor_l.contains(owner) {
                score += 35;
            }
            if anchor_l.contains(&format!("-{}-{}", owner, member))
                || anchor_l.contains(&format!("{}-{}", owner, member))
            {
                score += 50;
            }
            if anchor_l.ends_with(&format!("-{}", member)) {
                score += 10;
            }
            if title_l == member {
                score += 50;
            } else if title_l.contains(member) {
                score += 20;
            }
            if section_type == "idl" {
                score += 10;
            } else if section_type == "definition" {
                score += 5;
            }

            // Deprioritize candidates that don't mention owner at all.
            if !anchor_l.contains(owner) {
                score -= 20;
            }

            if score > 0 {
                candidates.push(Candidate {
                    spec,
                    anchor,
                    title,
                    section_type,
                    score,
                });
            }
        }
    } else {
        while let Some(row) = rows.next()? {
            let spec: String = row.get(0)?;
            let anchor: String = row.get(1)?;
            let title: Option<String> = row.get(2)?;
            let section_type: String = row.get(3)?;
            let anchor_l = anchor.to_ascii_lowercase();
            let title_l = title.as_deref().unwrap_or("").to_ascii_lowercase();

            let mut score = 0;
            if anchor_l == q {
                score += 100;
            }
            if title_l == q {
                score += 90;
            }
            if anchor_l.contains(&q) {
                score += 40;
            }
            if title_l.contains(&q) {
                score += 30;
            }
            if section_type == "idl" || section_type == "definition" {
                score += 5;
            }

            if score > 0 {
                candidates.push(Candidate {
                    spec,
                    anchor,
                    title,
                    section_type,
                    score,
                });
            }
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.spec.cmp(&b.spec))
            .then(a.anchor.cmp(&b.anchor))
    });
    let mut seen: HashSet<(String, String)> = HashSet::new();
    candidates.retain(|c| seen.insert((c.spec.clone(), c.anchor.clone())));
    candidates.truncate(limit);

    Ok(candidates)
}

fn find_references_from_conn(
    conn: &Connection,
    exact_target: Option<(String, String)>,
    query: &str,
    direction: &str,
    limit: usize,
) -> Result<model::FindReferencesResult> {
    let dir = parse_ref_direction(direction)?;
    let mut matches = Vec::new();
    let exact_mode = exact_target.is_some();

    let candidates = if let Some((spec, anchor)) = exact_target {
        let (title, section_type) =
            section_meta(conn, &spec, &anchor)?.unwrap_or((None, Some("unknown".to_string())));
        vec![Candidate {
            spec,
            anchor,
            title,
            section_type: section_type.unwrap_or_else(|| "unknown".to_string()),
            score: i32::MAX,
        }]
    } else {
        resolve_find_references_candidates(conn, query, limit)?
    };

    for candidate in candidates {
        let outgoing = if dir == RefDirection::Outgoing || dir == RefDirection::Both {
            Some(
                outgoing_refs_for_node(conn, &candidate.spec, &candidate.anchor)?
                    .into_iter()
                    .map(|(to_spec, to_anchor)| model::RefEntry {
                        spec: to_spec,
                        anchor: to_anchor,
                    })
                    .collect(),
            )
        } else {
            None
        };

        let incoming = if dir == RefDirection::Incoming || dir == RefDirection::Both {
            Some(
                db::queries::get_incoming_refs(conn, &candidate.spec, &candidate.anchor)?
                    .into_iter()
                    .map(|(from_spec, from_anchor)| model::RefEntry {
                        spec: from_spec,
                        anchor: from_anchor,
                    })
                    .collect(),
            )
        } else {
            None
        };

        matches.push(model::FindReferencesMatch {
            spec: candidate.spec,
            anchor: candidate.anchor,
            title: candidate.title,
            section_type: candidate.section_type,
            resolution: if exact_mode {
                "exact".to_string()
            } else {
                "heuristic".to_string()
            },
            outgoing,
            incoming,
        });
    }

    Ok(model::FindReferencesResult {
        query: query.to_string(),
        direction: direction.to_ascii_lowercase(),
        matches,
    })
}

fn normalize_idl_query(query: &str) -> String {
    let trimmed = query.trim();
    if let Some((owner, member)) = trimmed.rsplit_once('.') {
        let owner = owner.trim();
        let member = member.trim().trim_end_matches("()");
        if owner.is_empty() {
            return member.to_string();
        }
        return format!("{owner}.{member}");
    }
    trimmed.trim_end_matches("()").to_string()
}

fn query_idl_from_conn(
    conn: &Connection,
    query: &str,
    spec_filter: Option<&str>,
    limit: usize,
) -> Result<model::IdlResult> {
    let mut entries = Vec::new();

    // Exact anchor lookup
    if let Ok((spec_name, anchor)) = parse_spec_anchor(query) {
        if spec_filter.is_some() && spec_filter != Some(spec_name.as_str()) {
            return Ok(model::IdlResult {
                query: query.to_string(),
                matches: vec![],
            });
        }

        let mut stmt = conn.prepare(
            "SELECT sp.name, d.anchor, d.kind, d.name, d.owner, d.canonical_name, d.idl_text, s.title
             FROM idl_defs d
             JOIN snapshots sn ON d.snapshot_id = sn.id
             JOIN specs sp ON sn.spec_id = sp.id
             LEFT JOIN sections s ON s.snapshot_id = d.snapshot_id AND s.anchor = d.anchor
             WHERE sp.name = ?1 AND d.anchor = ?2
             ORDER BY d.kind
             LIMIT ?3",
        )?;

        let rows = stmt
            .query_map((spec_name, anchor, limit), |row| {
                Ok(model::IdlEntry {
                    spec: row.get(0)?,
                    anchor: row.get(1)?,
                    kind: row.get(2)?,
                    name: row.get(3)?,
                    owner: row.get(4)?,
                    canonical_name: row.get(5)?,
                    idl_text: row.get(6)?,
                    title: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        entries.extend(rows);
    } else {
        let normalized = normalize_idl_query(query).to_ascii_lowercase();
        if normalized.is_empty() {
            return Ok(model::IdlResult {
                query: query.to_string(),
                matches: vec![],
            });
        }
        let like = format!("%{}%", normalized);

        let sql_with_spec = "SELECT sp.name, d.anchor, d.kind, d.name, d.owner, d.canonical_name, d.idl_text, s.title,
                CASE
                    WHEN LOWER(d.canonical_name) = ?1 THEN 100
                    WHEN LOWER(d.name) = ?1 THEN 95
                    WHEN LOWER(d.anchor) = ?1 THEN 90
                    WHEN LOWER(d.canonical_name) LIKE ?2 THEN 80
                    WHEN LOWER(d.name) LIKE ?2 THEN 70
                    ELSE 0
                END AS score
             FROM idl_defs d
             JOIN snapshots sn ON d.snapshot_id = sn.id
             JOIN specs sp ON sn.spec_id = sp.id
             LEFT JOIN sections s ON s.snapshot_id = d.snapshot_id AND s.anchor = d.anchor
             WHERE sp.name = ?3
               AND (LOWER(d.canonical_name) = ?1 OR LOWER(d.name) = ?1 OR LOWER(d.anchor) = ?1
                    OR LOWER(d.canonical_name) LIKE ?2 OR LOWER(d.name) LIKE ?2)
             ORDER BY score DESC, sp.name, d.canonical_name
             LIMIT ?4";

        let sql_without_spec = "SELECT sp.name, d.anchor, d.kind, d.name, d.owner, d.canonical_name, d.idl_text, s.title,
                CASE
                    WHEN LOWER(d.canonical_name) = ?1 THEN 100
                    WHEN LOWER(d.name) = ?1 THEN 95
                    WHEN LOWER(d.anchor) = ?1 THEN 90
                    WHEN LOWER(d.canonical_name) LIKE ?2 THEN 80
                    WHEN LOWER(d.name) LIKE ?2 THEN 70
                    ELSE 0
                END AS score
             FROM idl_defs d
             JOIN snapshots sn ON d.snapshot_id = sn.id
             JOIN specs sp ON sn.spec_id = sp.id
             LEFT JOIN sections s ON s.snapshot_id = d.snapshot_id AND s.anchor = d.anchor
             WHERE (LOWER(d.canonical_name) = ?1 OR LOWER(d.name) = ?1 OR LOWER(d.anchor) = ?1
                    OR LOWER(d.canonical_name) LIKE ?2 OR LOWER(d.name) LIKE ?2)
             ORDER BY score DESC, sp.name, d.canonical_name
             LIMIT ?3";

        let mut stmt = conn.prepare(if spec_filter.is_some() {
            sql_with_spec
        } else {
            sql_without_spec
        })?;

        if let Some(spec_name) = spec_filter {
            let rows = stmt
                .query_map((normalized, like, spec_name, limit), |row| {
                    Ok(model::IdlEntry {
                        spec: row.get(0)?,
                        anchor: row.get(1)?,
                        kind: row.get(2)?,
                        name: row.get(3)?,
                        owner: row.get(4)?,
                        canonical_name: row.get(5)?,
                        idl_text: row.get(6)?,
                        title: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            entries.extend(rows);
        } else {
            let rows = stmt
                .query_map((normalized, like, limit), |row| {
                    Ok(model::IdlEntry {
                        spec: row.get(0)?,
                        anchor: row.get(1)?,
                        kind: row.get(2)?,
                        name: row.get(3)?,
                        owner: row.get(4)?,
                        canonical_name: row.get(5)?,
                        idl_text: row.get(6)?,
                        title: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            entries.extend(rows);
        }
    }

    Ok(model::IdlResult {
        query: query.to_string(),
        matches: entries,
    })
}

/// Build a cross-reference graph rooted at SPEC#anchor from currently indexed specs.
pub async fn graph_section(
    spec_anchor: &str,
    direction: &str,
    max_depth: usize,
    max_nodes: usize,
    include: &[String],
    exclude: &[String],
    same_spec_only: bool,
) -> Result<model::GraphResult> {
    let (spec_name, anchor) = parse_spec_anchor(spec_anchor)?;
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    let (_snapshot_id, spec_name) =
        ensure_indexed_for_spec_name(&conn, &registry, &spec_name).await?;

    let filters = GraphFilters {
        include: include.to_vec(),
        exclude: exclude.to_vec(),
        same_spec_only,
    };

    build_graph_from_conn(
        &conn, &spec_name, &anchor, direction, max_depth, max_nodes, &filters,
    )
}

/// Query dedicated WebIDL definitions.
///
/// `query` supports:
/// - exact anchor: `SPEC#anchor` or full URL
/// - canonical name: `Interface.member`, `Interface.method()`, `Interface`
pub async fn query_idl(
    query: &str,
    spec_filter: Option<&str>,
    limit: usize,
) -> Result<model::IdlResult> {
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    if let Some(spec_name) = spec_filter {
        let _ = ensure_indexed_for_spec_name(&conn, &registry, spec_name).await?;
    } else if let Ok((spec_name, _)) = parse_spec_anchor(query) {
        let _ = ensure_indexed_for_spec_name(&conn, &registry, &spec_name).await?;
    }

    query_idl_from_conn(&conn, query, spec_filter, limit)
}

/// Find incoming/outgoing references for SPEC#anchor or a shorthand query (e.g. Window.navigation).
pub async fn find_references(
    target: &str,
    direction: &str,
    limit: usize,
) -> Result<model::FindReferencesResult> {
    let conn = db::open_or_create_db()?;
    let registry = spec_registry::SpecRegistry::new();

    let exact_target = match parse_spec_anchor(target) {
        Ok((spec_name, anchor)) => {
            let (_snapshot_id, canonical_spec_name) =
                ensure_indexed_for_spec_name(&conn, &registry, &spec_name).await?;
            Some((canonical_spec_name, anchor))
        }
        Err(_) => None,
    };

    find_references_from_conn(&conn, exact_target, target, direction, limit)
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
        let (canonical_name, base_url, provider) =
            resolve_spec_metadata(&conn, &registry, spec_name)?;
        let snapshot_id =
            fetch::update_if_needed(&conn, &canonical_name, &base_url, &provider, force).await?;
        results.push((canonical_name, snapshot_id));
    } else {
        // Update all indexed/discovered specs.
        let specs = db::queries::list_specs(&conn)?;
        let all_results = fetch::update_all_specs(&conn, &specs, force).await;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::write;
    use crate::model::{ParsedReference, ParsedSection, SectionType};
    use rusqlite::Connection;

    fn default_graph_filters() -> GraphFilters {
        GraphFilters {
            include: vec![],
            exclude: vec![],
            same_spec_only: false,
        }
    }

    fn setup_reference_graph_db() -> Connection {
        let conn = db::open_test_db().unwrap();

        let html_spec_id =
            write::insert_or_get_spec(&conn, "HTML", "https://html.spec.whatwg.org", "whatwg")
                .unwrap();
        let dom_spec_id =
            write::insert_or_get_spec(&conn, "DOM", "https://dom.spec.whatwg.org", "whatwg")
                .unwrap();
        let url_spec_id =
            write::insert_or_get_spec(&conn, "URL", "https://url.spec.whatwg.org", "whatwg")
                .unwrap();

        let html_snapshot =
            write::insert_snapshot(&conn, html_spec_id, "sha-html", "2026-01-01T00:00:00Z")
                .unwrap();
        let dom_snapshot =
            write::insert_snapshot(&conn, dom_spec_id, "sha-dom", "2026-01-01T00:00:00Z").unwrap();
        let url_snapshot =
            write::insert_snapshot(&conn, url_spec_id, "sha-url", "2026-01-01T00:00:00Z").unwrap();

        let html_sections = vec![
            ParsedSection {
                anchor: "navigate".to_string(),
                title: Some("navigate".to_string()),
                content_text: None,
                section_type: SectionType::Algorithm,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
            ParsedSection {
                anchor: "dom-window-navigation".to_string(),
                title: Some("navigation".to_string()),
                content_text: None,
                section_type: SectionType::Idl,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
            ParsedSection {
                anchor: "dom-worker-navigation".to_string(),
                title: Some("navigation".to_string()),
                content_text: None,
                section_type: SectionType::Idl,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
            ParsedSection {
                anchor: "some-consumer".to_string(),
                title: Some("consumer".to_string()),
                content_text: None,
                section_type: SectionType::Algorithm,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
            ParsedSection {
                anchor: "dom-window-navigation-helper".to_string(),
                title: Some("navigation helper".to_string()),
                content_text: None,
                section_type: SectionType::Algorithm,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
        ];
        write::insert_sections_bulk(&conn, html_snapshot, &html_sections).unwrap();

        let dom_sections = vec![ParsedSection {
            anchor: "concept-tree".to_string(),
            title: Some("tree".to_string()),
            content_text: None,
            section_type: SectionType::Definition,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: None,
        }];
        write::insert_sections_bulk(&conn, dom_snapshot, &dom_sections).unwrap();

        let url_sections = vec![
            ParsedSection {
                anchor: "concept-url".to_string(),
                title: Some("URL".to_string()),
                content_text: None,
                section_type: SectionType::Definition,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
            ParsedSection {
                anchor: "concept-relevant-global".to_string(),
                title: Some("relevant global object".to_string()),
                content_text: None,
                section_type: SectionType::Definition,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
        ];
        write::insert_sections_bulk(&conn, url_snapshot, &url_sections).unwrap();

        let html_refs = vec![
            ParsedReference {
                from_anchor: "navigate".to_string(),
                to_spec: "DOM".to_string(),
                to_anchor: "concept-tree".to_string(),
            },
            ParsedReference {
                from_anchor: "navigate".to_string(),
                to_spec: "URL".to_string(),
                to_anchor: "concept-url".to_string(),
            },
            ParsedReference {
                from_anchor: "some-consumer".to_string(),
                to_spec: "HTML".to_string(),
                to_anchor: "dom-window-navigation".to_string(),
            },
            ParsedReference {
                from_anchor: "dom-window-navigation".to_string(),
                to_spec: "URL".to_string(),
                to_anchor: "concept-url".to_string(),
            },
            ParsedReference {
                from_anchor: "dom-worker-navigation".to_string(),
                to_spec: "DOM".to_string(),
                to_anchor: "concept-tree".to_string(),
            },
            ParsedReference {
                from_anchor: "dom-window-navigation-helper".to_string(),
                to_spec: "HTML".to_string(),
                to_anchor: "navigate".to_string(),
            },
        ];
        write::insert_refs_bulk(&conn, html_snapshot, &html_refs).unwrap();

        let dom_refs = vec![ParsedReference {
            from_anchor: "concept-tree".to_string(),
            to_spec: "URL".to_string(),
            to_anchor: "concept-url".to_string(),
        }];
        write::insert_refs_bulk(&conn, dom_snapshot, &dom_refs).unwrap();

        let url_refs = vec![ParsedReference {
            from_anchor: "concept-relevant-global".to_string(),
            to_spec: "HTML".to_string(),
            to_anchor: "dom-window-navigation-helper".to_string(),
        }];
        write::insert_refs_bulk(&conn, url_snapshot, &url_refs).unwrap();

        let idl_defs = vec![
            crate::model::ParsedIdlDefinition {
                anchor: "dom-window".to_string(),
                name: "Window".to_string(),
                owner: None,
                kind: "interface".to_string(),
                canonical_name: "Window".to_string(),
                idl_text: Some("interface Window { ... };".to_string()),
            },
            crate::model::ParsedIdlDefinition {
                anchor: "dom-window-navigation".to_string(),
                name: "navigation".to_string(),
                owner: Some("Window".to_string()),
                kind: "attribute".to_string(),
                canonical_name: "Window.navigation".to_string(),
                idl_text: Some(
                    "interface Window { attribute Navigation navigation; };".to_string(),
                ),
            },
            crate::model::ParsedIdlDefinition {
                anchor: "dom-window-open".to_string(),
                name: "open(url)".to_string(),
                owner: Some("Window".to_string()),
                kind: "method".to_string(),
                canonical_name: "Window.open".to_string(),
                idl_text: Some("interface Window { undefined open(DOMString url); };".to_string()),
            },
        ];
        write::insert_idl_defs_bulk(&conn, html_snapshot, &idl_defs).unwrap();

        conn
    }

    #[test]
    fn parse_spec_anchor_classic_format() {
        let (spec, anchor) = parse_spec_anchor("HTML#navigate").unwrap();
        assert_eq!(spec, "HTML");
        assert_eq!(anchor, "navigate");
    }

    #[test]
    fn parse_spec_anchor_url_format() {
        let (spec, anchor) = parse_spec_anchor("https://html.spec.whatwg.org/#navigate").unwrap();
        assert_eq!(spec, "HTML");
        assert_eq!(anchor, "navigate");
    }

    #[test]
    fn parse_spec_anchor_url_dom() {
        let (spec, anchor) =
            parse_spec_anchor("https://dom.spec.whatwg.org/#concept-tree").unwrap();
        assert_eq!(spec, "DOM");
        assert_eq!(anchor, "concept-tree");
    }

    #[test]
    fn parse_spec_anchor_url_without_scheme() {
        let (spec, anchor) = parse_spec_anchor("html.spec.whatwg.org/#navigate").unwrap();
        assert_eq!(spec, "HTML");
        assert_eq!(anchor, "navigate");
    }

    #[test]
    fn parse_spec_anchor_auto_whitelisted_url() {
        let (spec, anchor) =
            parse_spec_anchor("https://wicg.github.io/permissions-policy/#permissions-policy")
                .unwrap();
        assert_eq!(spec, "PERMISSIONS-POLICY");
        assert_eq!(anchor, "permissions-policy");
    }

    #[test]
    fn parse_spec_anchor_unknown_url() {
        let result = parse_spec_anchor("https://example.com/#foo");
        assert!(result.is_err());
    }

    #[test]
    fn parse_spec_anchor_invalid() {
        let result = parse_spec_anchor("no-hash");
        assert!(result.is_err());
    }

    #[test]
    fn spec_urls_returns_without_panicking() {
        let urls = spec_urls();
        assert!(urls.iter().all(|entry| !entry.spec.is_empty()));
        assert!(urls.iter().all(|entry| entry.base_url.starts_with("http")));
    }

    #[test]
    fn sanitize_for_fts_handles_punctuation() {
        let sanitized = sanitize_for_fts("Where is attribute reflection defined?");
        assert_eq!(
            sanitized.as_deref(),
            Some("Where is attribute reflection defined")
        );
    }

    #[test]
    fn sanitize_for_fts_returns_none_when_no_terms() {
        assert_eq!(sanitize_for_fts("???"), None);
    }

    #[test]
    fn detects_fts_syntax_error_message() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::Unknown,
                extended_code: 1,
            },
            Some("fts5: syntax error near \"?\"".to_string()),
        );
        assert!(is_fts_syntax_error(&err));
    }

    #[test]
    fn graph_outgoing_depth_one() {
        let conn = setup_reference_graph_db();
        let graph = build_graph_from_conn(
            &conn,
            "HTML",
            "navigate",
            "outgoing",
            1,
            50,
            &default_graph_filters(),
        )
        .unwrap();

        assert_eq!(graph.root.spec, "HTML");
        assert_eq!(graph.root.anchor, "navigate");
        assert_eq!(graph.edges.len(), 2);
        assert!(graph
            .edges
            .iter()
            .any(|e| e.from == "HTML#navigate" && e.to == "DOM#concept-tree"));
        assert!(graph
            .edges
            .iter()
            .any(|e| e.from == "HTML#navigate" && e.to == "URL#concept-url"));
    }

    #[test]
    fn graph_outgoing_depth_two_follows_transitive_edges() {
        let conn = setup_reference_graph_db();
        let graph = build_graph_from_conn(
            &conn,
            "HTML",
            "navigate",
            "outgoing",
            2,
            50,
            &default_graph_filters(),
        )
        .unwrap();

        assert!(graph
            .edges
            .iter()
            .any(|e| { e.from == "DOM#concept-tree" && e.to == "URL#concept-url" }));
    }

    #[test]
    fn find_references_exact_anchor_incoming() {
        let conn = setup_reference_graph_db();
        let result = find_references_from_conn(
            &conn,
            Some(("HTML".to_string(), "dom-window-navigation".to_string())),
            "HTML#dom-window-navigation",
            "incoming",
            10,
        )
        .unwrap();

        assert_eq!(result.matches.len(), 1);
        let m = &result.matches[0];
        assert_eq!(m.resolution, "exact");
        assert!(m
            .incoming
            .as_ref()
            .unwrap()
            .iter()
            .any(|r| r.spec == "HTML" && r.anchor == "some-consumer"));
    }

    #[test]
    fn find_references_property_shorthand_prefers_window_navigation() {
        let conn = setup_reference_graph_db();
        let result =
            find_references_from_conn(&conn, None, "Window.navigation", "incoming", 10).unwrap();

        assert!(!result.matches.is_empty());
        let first = &result.matches[0];
        assert_eq!(first.spec, "HTML");
        assert_eq!(first.anchor, "dom-window-navigation");
        assert_eq!(first.resolution, "heuristic");
    }

    #[test]
    fn graph_mermaid_render_contains_nodes_and_edges() {
        let conn = setup_reference_graph_db();
        let graph = build_graph_from_conn(
            &conn,
            "HTML",
            "navigate",
            "outgoing",
            1,
            50,
            &default_graph_filters(),
        )
        .unwrap();
        let mermaid = crate::format::graph_mermaid(&graph);

        assert!(mermaid.contains("graph TD"));
        assert!(mermaid.contains("HTML#navigate"));
        assert!(mermaid.contains("-->"));
        assert!(mermaid.contains("<br>"));
        assert!(!mermaid.contains("\\n"));
    }

    #[test]
    fn graph_dot_render_contains_nodes_and_edges() {
        let conn = setup_reference_graph_db();
        let graph = build_graph_from_conn(
            &conn,
            "HTML",
            "navigate",
            "outgoing",
            1,
            50,
            &default_graph_filters(),
        )
        .unwrap();
        let dot = crate::format::graph_dot(&graph);

        assert!(dot.contains("digraph"));
        assert!(dot.contains("\"HTML#navigate\""));
        assert!(dot.contains("->"));
    }

    #[test]
    fn graph_same_spec_only_keeps_only_root_spec_nodes() {
        let conn = setup_reference_graph_db();
        let mut filters = default_graph_filters();
        filters.same_spec_only = true;

        let graph =
            build_graph_from_conn(&conn, "HTML", "navigate", "outgoing", 2, 50, &filters).unwrap();

        assert!(graph.nodes.iter().all(|n| n.spec == "HTML"));
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn graph_wildcard_include_filters_nodes() {
        let conn = setup_reference_graph_db();
        let mut filters = default_graph_filters();
        filters.include = vec!["*concept-*".to_string()];

        let graph =
            build_graph_from_conn(&conn, "HTML", "navigate", "outgoing", 1, 50, &filters).unwrap();

        assert!(graph.nodes.iter().any(|n| n.id == "HTML#navigate"));
        assert!(graph.nodes.iter().any(|n| n.id == "DOM#concept-tree"));
        assert!(graph.nodes.iter().any(|n| n.id == "URL#concept-url"));
        assert!(!graph
            .nodes
            .iter()
            .any(|n| n.id == "HTML#dom-window-navigation"));
    }

    #[test]
    fn graph_regex_exclude_filters_nodes() {
        let conn = setup_reference_graph_db();
        let mut filters = default_graph_filters();
        filters.exclude = vec!["re:^URL#".to_string()];

        let graph =
            build_graph_from_conn(&conn, "HTML", "navigate", "outgoing", 1, 50, &filters).unwrap();

        assert!(!graph.nodes.iter().any(|n| n.id == "URL#concept-url"));
        assert!(graph.nodes.iter().any(|n| n.id == "DOM#concept-tree"));
        assert!(!graph.edges.iter().any(|e| e.to == "URL#concept-url"));
    }

    #[test]
    fn graph_filters_prune_disconnected_components() {
        let conn = setup_reference_graph_db();
        let mut filters = default_graph_filters();
        filters.include = vec!["*concept-*".to_string()];

        let graph =
            build_graph_from_conn(&conn, "HTML", "navigate", "incoming", 2, 50, &filters).unwrap();

        // concept-relevant-global exists via a non-matching intermediary.
        // The intermediary should be kept as a bridge node.
        assert!(graph.nodes.iter().any(|n| n.id == "HTML#navigate"));
        assert!(graph
            .nodes
            .iter()
            .any(|n| n.id == "URL#concept-relevant-global"));
        assert!(graph
            .nodes
            .iter()
            .any(|n| n.id == "HTML#dom-window-navigation-helper"));
        let bridge = graph
            .nodes
            .iter()
            .find(|n| n.id == "HTML#dom-window-navigation-helper")
            .unwrap();
        assert_eq!(bridge.filter_role.as_deref(), Some("bridge"));
        assert!(graph.edges.iter().any(|e| {
            e.from == "URL#concept-relevant-global" && e.to == "HTML#dom-window-navigation-helper"
        }));
        assert!(graph
            .edges
            .iter()
            .any(|e| { e.from == "HTML#dom-window-navigation-helper" && e.to == "HTML#navigate" }));
    }

    #[test]
    fn graph_drops_self_referencing_edges() {
        let conn = setup_reference_graph_db();
        let dom_snapshot = db::queries::get_snapshot(&conn, "DOM").unwrap().unwrap();
        write::insert_refs_bulk(
            &conn,
            dom_snapshot,
            &[ParsedReference {
                from_anchor: "concept-tree".to_string(),
                to_spec: "DOM".to_string(),
                to_anchor: "concept-tree".to_string(),
            }],
        )
        .unwrap();

        let graph = build_graph_from_conn(
            &conn,
            "DOM",
            "concept-tree",
            "outgoing",
            1,
            50,
            &default_graph_filters(),
        )
        .unwrap();

        assert!(
            !graph
                .edges
                .iter()
                .any(|e| e.from == "DOM#concept-tree" && e.to == "DOM#concept-tree"),
            "Self-loop edges should be removed from graph output"
        );
    }

    #[test]
    fn graph_mermaid_styles_bridge_nodes() {
        let conn = setup_reference_graph_db();
        let mut filters = default_graph_filters();
        filters.include = vec!["*concept-*".to_string()];

        let graph =
            build_graph_from_conn(&conn, "HTML", "navigate", "incoming", 2, 50, &filters).unwrap();
        let mermaid = crate::format::graph_mermaid(&graph);

        assert!(mermaid.contains("classDef bridge"));
        assert!(mermaid.contains("classDef root"));
        assert!(mermaid.contains("class "));
        assert!(
            !mermaid.contains("classDef bridge stroke-dasharray: 5 5;"),
            "Mermaid classDef must not end with semicolon"
        );
    }

    #[test]
    fn graph_dot_label_newline_not_double_escaped() {
        let conn = setup_reference_graph_db();
        let graph = build_graph_from_conn(
            &conn,
            "HTML",
            "navigate",
            "outgoing",
            1,
            50,
            &default_graph_filters(),
        )
        .unwrap();
        let dot = crate::format::graph_dot(&graph);

        // navigate has title "navigate" → DOT label must use a single \n (backslash-n),
        // not the double-escaped \\n that would appear if escape runs on the combined string.
        assert!(
            dot.contains("[label=\"HTML#navigate\\nnavigate\"]"),
            "DOT label should use single \\n as line separator, got:\n{}",
            dot
        );
    }

    #[test]
    fn graph_max_nodes_truncation() {
        let conn = setup_reference_graph_db();
        // navigate has 2 outgoing refs; with max_nodes=2 one neighbour must be dropped
        let graph = build_graph_from_conn(
            &conn,
            "HTML",
            "navigate",
            "outgoing",
            2,
            2,
            &default_graph_filters(),
        )
        .unwrap();
        assert!(
            graph.truncated,
            "graph should be truncated when max_nodes is hit"
        );
        assert!(graph.nodes.len() <= 2);
    }

    #[test]
    fn graph_incoming_depth_one() {
        let conn = setup_reference_graph_db();
        let graph = build_graph_from_conn(
            &conn,
            "HTML",
            "dom-window-navigation",
            "incoming",
            1,
            50,
            &default_graph_filters(),
        )
        .unwrap();
        assert!(
            graph
                .edges
                .iter()
                .any(|e| e.from == "HTML#some-consumer" && e.to == "HTML#dom-window-navigation"),
            "incoming edge from some-consumer should be present"
        );
    }

    #[test]
    fn find_references_outgoing_direction() {
        let conn = setup_reference_graph_db();
        let result = find_references_from_conn(
            &conn,
            Some(("HTML".to_string(), "navigate".to_string())),
            "HTML#navigate",
            "outgoing",
            10,
        )
        .unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.direction, "outgoing");
        let m = &result.matches[0];
        let outgoing = m.outgoing.as_ref().unwrap();
        assert!(outgoing
            .iter()
            .any(|r| r.spec == "DOM" && r.anchor == "concept-tree"));
        assert!(outgoing
            .iter()
            .any(|r| r.spec == "URL" && r.anchor == "concept-url"));
        assert!(m.incoming.is_none());
    }

    #[test]
    fn query_idl_exact_anchor() {
        let conn = setup_reference_graph_db();
        let result = query_idl_from_conn(&conn, "HTML#dom-window-navigation", None, 10).unwrap();

        assert_eq!(result.matches.len(), 1);
        let m = &result.matches[0];
        assert_eq!(m.spec, "HTML");
        assert_eq!(m.kind, "attribute");
        assert_eq!(m.canonical_name, "Window.navigation");
    }

    #[test]
    fn query_idl_by_canonical_member() {
        let conn = setup_reference_graph_db();
        let result = query_idl_from_conn(&conn, "Window.navigation", None, 10).unwrap();

        assert!(!result.matches.is_empty());
        assert_eq!(result.matches[0].canonical_name, "Window.navigation");
    }

    #[test]
    fn query_idl_method_parentheses_normalized() {
        let conn = setup_reference_graph_db();
        let result = query_idl_from_conn(&conn, "Window.open()", None, 10).unwrap();

        assert!(!result.matches.is_empty());
        assert_eq!(result.matches[0].canonical_name, "Window.open");
    }
}
