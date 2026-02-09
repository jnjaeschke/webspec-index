use anyhow::Result;
use clap::{Parser, Subcommand};

mod db;
mod fetch;
mod format;
mod model;
mod parse;
mod provider;
mod spec_registry;

#[derive(Parser)]
#[command(name = "webspec-index")]
#[command(about = "Index and query WHATWG specifications")]
struct Cli {
    #[arg(long, global = true, default_value = "json")]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Debug)]
enum OutputFormat {
    Json,
    Markdown,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(OutputFormat::Json),
            "markdown" => Ok(OutputFormat::Markdown),
            _ => Err(format!("Invalid format: {}", s)),
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Query a specific section by anchor
    Query {
        /// Spec and anchor in format SPEC#anchor (e.g., HTML#navigate)
        spec_anchor: String,

        /// Optional commit SHA
        #[arg(long)]
        sha: Option<String>,
    },

    /// Check if an anchor exists
    Exists {
        /// Spec and anchor in format SPEC#anchor
        spec_anchor: String,
    },

    /// Find anchors matching a pattern
    Anchors {
        /// Pattern to match (glob syntax)
        pattern: String,

        /// Limit to specific spec
        #[arg(long)]
        spec: Option<String>,

        /// Maximum results
        #[arg(long, default_value = "50")]
        limit: usize,
    },

    /// Full-text search
    Search {
        /// Search query
        query: String,

        /// Limit to specific spec
        #[arg(long)]
        spec: Option<String>,

        /// Maximum results
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// List all headings in a spec
    List {
        /// Spec name
        spec: String,

        /// Optional commit SHA
        #[arg(long)]
        sha: Option<String>,
    },

    /// Get references to/from a section
    Refs {
        /// Spec and anchor in format SPEC#anchor
        spec_anchor: String,

        /// Direction: incoming, outgoing, or both
        #[arg(long, default_value = "both")]
        direction: String,

        /// Optional commit SHA
        #[arg(long)]
        sha: Option<String>,
    },

    /// Update specs to latest version
    Update {
        /// Specific spec to update
        #[arg(long)]
        spec: Option<String>,

        /// Force update even if recently checked
        #[arg(long)]
        force: bool,
    },

    /// Clear the database (removes all indexed data)
    ClearDb {
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

fn parse_spec_anchor(input: &str) -> Result<(String, String)> {
    let parts: Vec<&str> = input.split('#').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid format. Expected SPEC#anchor (e.g., HTML#navigate)");
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn print_json<T: serde::Serialize>(data: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(data)?);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Query { spec_anchor, sha } => {
            let (spec_name, anchor) = parse_spec_anchor(&spec_anchor)?;
            let conn = db::open_or_create_db()?;
            let registry = spec_registry::SpecRegistry::new();

            // Get spec info
            let spec = registry.find_spec(&spec_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;

            // Get snapshot and SHA
            let (snapshot_id, snapshot_sha) = if let Some(sha_str) = &sha {
                let id = db::queries::get_snapshot_by_sha(&conn, &spec_name, sha_str)?
                    .ok_or_else(|| anyhow::anyhow!("Snapshot not found for SHA: {}", sha_str))?;
                (id, sha_str.clone())
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
                    db::queries::get_section(&conn, snapshot_id, p).ok()?
                        .map(|s| model::NavEntry {
                            anchor: s.anchor,
                            title: s.title,
                        })
                }),
                prev: section.prev_anchor.as_ref().and_then(|p| {
                    db::queries::get_section(&conn, snapshot_id, p).ok()?
                        .map(|s| model::NavEntry {
                            anchor: s.anchor,
                            title: s.title,
                        })
                }),
                next: section.next_anchor.as_ref().and_then(|n| {
                    db::queries::get_section(&conn, snapshot_id, n).ok()?
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

            let result = model::QueryResult {
                spec: spec_name.clone(),
                sha: snapshot_sha,
                anchor: section.anchor,
                title: section.title,
                section_type: section.section_type.as_str().to_string(),
                content: section.content_text,
                navigation,
                outgoing_refs: outgoing,
                incoming_refs: incoming,
            };

            match &cli.format {
                OutputFormat::Json => print_json(&result)?,
                OutputFormat::Markdown => print!("{}", format::query(&result)),
            }
        }
        Commands::Exists { spec_anchor } => {
            let (spec_name, anchor) = parse_spec_anchor(&spec_anchor)?;
            let conn = db::open_or_create_db()?;
            let registry = spec_registry::SpecRegistry::new();

            // Get spec info
            let spec = registry.find_spec(&spec_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;

            // Ensure latest indexed
            let provider = registry.get_provider(spec)?;
            let snapshot_id = fetch::ensure_latest_indexed(&conn, spec, provider).await?;

            // Check if section exists
            let section = db::queries::get_section(&conn, snapshot_id, &anchor)?;
            let exists = section.is_some();
            let section_type = section.as_ref().map(|s| s.section_type.as_str().to_string());

            let result = model::ExistsResult {
                exists,
                spec: spec_name.clone(),
                anchor: anchor.clone(),
                section_type,
            };
            match &cli.format {
                OutputFormat::Json => print_json(&result)?,
                OutputFormat::Markdown => println!("{}", format::exists(&result)),
            }

            std::process::exit(if exists { 0 } else { 1 });
        }
        Commands::Anchors { pattern, spec: spec_filter, limit } => {
            let conn = db::open_or_create_db()?;

            // Convert glob pattern to SQL LIKE pattern
            let sql_pattern = pattern.replace('*', "%");

            // Find matching anchors - need to get more details
            // For now, query the sections directly with title and type
            let sql = if let Some(_) = &spec_filter {
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
            let results: Vec<(String, String, Option<String>, String)> = if let Some(spec) = &spec_filter {
                stmt.query_map((&sql_pattern, spec, limit), |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?.collect::<Result<Vec<_>, _>>()?
            } else {
                stmt.query_map((&sql_pattern, limit), |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?.collect::<Result<Vec<_>, _>>()?
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

            let result = model::AnchorsResult {
                pattern: pattern.clone(),
                results: entries,
            };
            match &cli.format {
                OutputFormat::Json => print_json(&result)?,
                OutputFormat::Markdown => print!("{}", format::anchors(&result)),
            }
        }
        Commands::Search { query: search_query, spec: spec_filter, limit } => {
            let conn = db::open_or_create_db()?;

            // Search sections using FTS5 - need to get title and section_type too
            let sql = if let Some(_) = &spec_filter {
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
            let results: Vec<(String, String, Option<String>, String, Option<String>)> = if let Some(spec) = &spec_filter {
                stmt.query_map((&search_query, spec, limit), |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
                })?.collect::<Result<Vec<_>, _>>()?
            } else {
                stmt.query_map((&search_query, limit), |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
                })?.collect::<Result<Vec<_>, _>>()?
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

            let result = model::SearchResult {
                query: search_query.clone(),
                results: entries,
            };
            match &cli.format {
                OutputFormat::Json => print_json(&result)?,
                OutputFormat::Markdown => print!("{}", format::search(&result)),
            }
        }
        Commands::List { spec: spec_name, sha } => {
            let conn = db::open_or_create_db()?;
            let registry = spec_registry::SpecRegistry::new();

            // Get spec info
            let spec = registry.find_spec(&spec_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;

            // Get snapshot
            let snapshot_id = if let Some(sha_str) = sha {
                db::queries::get_snapshot_by_sha(&conn, &spec_name, &sha_str)?
                    .ok_or_else(|| anyhow::anyhow!("Snapshot not found for SHA: {}", sha_str))?
            } else {
                // Ensure latest indexed
                let provider = registry.get_provider(spec)?;
                fetch::ensure_latest_indexed(&conn, spec, provider).await?
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

            match &cli.format {
                OutputFormat::Json => print_json(&entries)?,
                OutputFormat::Markdown => print!("{}", format::list(&entries)),
            }
        }
        Commands::Refs { spec_anchor, direction, sha } => {
            let (spec_name, anchor) = parse_spec_anchor(&spec_anchor)?;
            let conn = db::open_or_create_db()?;
            let registry = spec_registry::SpecRegistry::new();

            // Get spec info
            let spec = registry.find_spec(&spec_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;

            // Get snapshot
            let snapshot_id = if let Some(sha_str) = sha {
                db::queries::get_snapshot_by_sha(&conn, &spec_name, &sha_str)?
                    .ok_or_else(|| anyhow::anyhow!("Snapshot not found for SHA: {}", sha_str))?
            } else {
                // Ensure latest indexed
                let provider = registry.get_provider(spec)?;
                fetch::ensure_latest_indexed(&conn, spec, provider).await?
            };

            // Get references based on direction
            let outgoing = if direction == "outgoing" || direction == "both" {
                let out_refs = db::queries::get_outgoing_refs(&conn, snapshot_id, &anchor)?;
                Some(out_refs
                    .iter()
                    .map(|(to_spec, to_anchor)| model::RefEntry {
                        spec: to_spec.clone(),
                        anchor: to_anchor.clone(),
                    })
                    .collect())
            } else {
                None
            };

            let incoming = if direction == "incoming" || direction == "both" {
                let in_refs = db::queries::get_incoming_refs(&conn, snapshot_id, &spec_name, &anchor)?;
                Some(in_refs
                    .iter()
                    .map(|(from_spec, from_anchor)| model::RefEntry {
                        spec: from_spec.clone(),
                        anchor: from_anchor.clone(),
                    })
                    .collect())
            } else {
                None
            };

            let result = model::RefsResult {
                anchor: anchor.clone(),
                direction: direction.clone(),
                outgoing,
                incoming,
            };
            match &cli.format {
                OutputFormat::Json => print_json(&result)?,
                OutputFormat::Markdown => print!("{}", format::refs(&result)),
            }
        }
        Commands::Update { spec: spec_filter, force } => {
            let conn = db::open_or_create_db()?;
            let registry = spec_registry::SpecRegistry::new();

            if let Some(spec_name) = spec_filter {
                // Update single spec
                let spec = registry.find_spec(&spec_name)
                    .ok_or_else(|| anyhow::anyhow!("Unknown spec: {}", spec_name))?;
                let provider = registry.get_provider(spec)?;

                match fetch::update_if_needed(&conn, spec, provider, force).await? {
                    Some(snapshot_id) => {
                        println!("Updated {} (snapshot_id: {})", spec_name, snapshot_id);
                    }
                    None => {
                        println!("{} is already up to date", spec_name);
                    }
                }
            } else {
                // Update all specs
                let results = fetch::update_all_specs(&conn, &registry, force).await;

                for (spec_name, result) in results {
                    match result {
                        Ok(Some(snapshot_id)) => {
                            println!("Updated {} (snapshot_id: {})", spec_name, snapshot_id);
                        }
                        Ok(None) => {
                            println!("{} is already up to date", spec_name);
                        }
                        Err(e) => {
                            eprintln!("Failed to update {}: {}", spec_name, e);
                        }
                    }
                }
            }
        }
        Commands::ClearDb { yes } => {
            let db_path = db::get_db_path();

            if !db_path.exists() {
                println!("Database does not exist: {}", db_path.display());
                return Ok(());
            }

            if !yes {
                use std::io::{self, Write};
                println!("This will delete: {}", db_path.display());
                print!("Continue? [y/N] ");
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            std::fs::remove_file(&db_path)?;
            println!("Database cleared: {}", db_path.display());
        }
    }

    Ok(())
}
