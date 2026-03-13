use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use moz_cli_version_check::VersionChecker;

use webspec_index::{format, model};

#[derive(Parser, Debug)]
#[command(
    name = "webspec-index",
    version,
    about = "Query WHATWG/W3C/TC39 web specifications",
    long_about = "A command-line tool for querying web specification sections, algorithms, \
        and cross-references.\n\n\
        Indexes specs from WHATWG (HTML, DOM, URL, Fetch, …), W3C (CSS, Geometry, …), \
        and TC39 (ECMAScript). Specs are fetched and cached locally on first use.\n\n\
        Examples:\n  \
        webspec-index query HTML#navigate\n  \
        webspec-index search \"tree order\" --spec DOM\n  \
        webspec-index anchors \"*-tree\" --spec DOM\n  \
        webspec-index refs HTML#navigate --direction incoming\n  \
        webspec-index list DOM\n  \
        webspec-index exists HTML#navigate"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        default_value = "json",
        help = "Output format",
        long_help = "Output format.\n  json     — JSON (default, best for programmatic use)\n  markdown — Human-readable markdown"
    )]
    format: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(ValueEnum, Clone, Debug)]
enum OutputFormat {
    Json,
    Markdown,
}

#[derive(ValueEnum, Clone, Debug)]
enum GraphOutputFormat {
    Json,
    Markdown,
    Mermaid,
    Dot,
}

#[derive(ValueEnum, Clone, Debug)]
enum AnalyzeFormat {
    Json,
    Searchfox,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Query a specific section in a specification
    ///
    /// Returns complete section information including content, navigation
    /// (parent/prev/next/children), and cross-references.
    #[command(long_about = "Query a specific section in a specification.\n\n\
        Returns complete section information including content, navigation\n\
        (parent/prev/next/children), and cross-references.\n\n\
        The argument can be SPEC#anchor or a full spec URL:\n  \
        webspec-index query HTML#navigate\n  \
        webspec-index query \"https://html.spec.whatwg.org/#navigate\"")]
    Query {
        /// Section identifier: SPEC#anchor or full URL
        spec_anchor: String,
    },

    /// Full-text search across specifications
    #[command(long_about = "Full-text search across indexed specifications.\n\n\
        Uses SQLite FTS5 for fast text search. Results include snippets\n\
        showing matching context.\n\n\
        Examples:\n  \
        webspec-index search \"tree order\"\n  \
        webspec-index search \"navigate\" --spec HTML --limit 5")]
    Search {
        /// Search query string
        query: String,

        #[arg(long, short, help = "Limit search to a specific spec (e.g. HTML, DOM)")]
        spec: Option<String>,

        #[arg(long, short, default_value = "20", help = "Maximum number of results")]
        limit: usize,
    },

    /// Check if a section exists (exit code 0 = found, 1 = not found)
    Exists {
        /// Section identifier: SPEC#anchor or full URL
        spec_anchor: String,
    },

    /// Find anchors matching a glob pattern
    #[command(long_about = "Find anchors matching a glob pattern.\n\n\
        Uses * as wildcard. Searches across all indexed specs unless\n\
        --spec is given.\n\n\
        Examples:\n  \
        webspec-index anchors \"*-tree\" --spec DOM\n  \
        webspec-index anchors \"concept-*\"")]
    Anchors {
        /// Glob pattern (e.g. "*-tree", "concept-*")
        pattern: String,

        #[arg(long, short, help = "Limit to a specific spec")]
        spec: Option<String>,

        #[arg(long, short, default_value = "50", help = "Maximum number of results")]
        limit: usize,
    },

    /// List all headings in a specification
    List {
        /// Spec name (e.g. HTML, DOM, CSS-GRID)
        spec: String,
    },

    /// Get cross-references for a section
    #[command(long_about = "Get cross-references for a section.\n\n\
        Shows which other spec sections reference this one (incoming)\n\
        and which sections this one references (outgoing).\n\n\
        Examples:\n  \
        webspec-index refs HTML#navigate\n  \
        webspec-index refs HTML#navigate --direction incoming")]
    Refs {
        /// Section identifier: SPEC#anchor or full URL
        spec_anchor: String,

        #[arg(
            long,
            short,
            default_value = "both",
            help = "Reference direction: incoming, outgoing, or both"
        )]
        direction: String,
    },

    /// Find references for SPEC#anchor or shorthand (e.g. Window.navigation)
    #[command(long_about = "Find references for a target section.\n\n\
        Target can be SPEC#anchor (exact), full URL, or shorthand such as\n\
        Interface.member (heuristic match against indexed sections).\n\n\
        Examples:\n  \
        webspec-index find-references HTML#navigate\n  \
        webspec-index find-references Window.navigation --direction incoming")]
    FindReferences {
        /// Target identifier: SPEC#anchor, full URL, or shorthand (e.g. Window.navigation)
        target: String,

        #[arg(
            long,
            short,
            default_value = "incoming",
            help = "Reference direction: incoming, outgoing, or both"
        )]
        direction: String,

        #[arg(long, short, default_value = "10", help = "Maximum number of matches")]
        limit: usize,
    },

    /// Build a cross-reference graph rooted at a section
    #[command(
        long_about = "Build a cross-reference graph rooted at SPEC#anchor.\n\n\
        Traverses indexed references up to --max-depth and returns a graph.\n\
        Output formats: json, markdown, mermaid, dot.\n\n\
        Examples:\n  \
        webspec-index graph HTML#navigate --direction outgoing --max-depth 2\n  \
        webspec-index graph HTML#navigate --graph-format mermaid"
    )]
    Graph {
        /// Root section identifier: SPEC#anchor or full URL
        spec_anchor: String,

        #[arg(
            long,
            short,
            default_value = "outgoing",
            help = "Traversal direction: incoming, outgoing, or both"
        )]
        direction: String,

        #[arg(long, default_value = "2", help = "Maximum traversal depth")]
        max_depth: usize,

        #[arg(long, default_value = "150", help = "Maximum number of graph nodes")]
        max_nodes: usize,

        #[arg(
            long = "include",
            help = "Include node id patterns (wildcard by default, or re:<regex>)",
            action = clap::ArgAction::Append
        )]
        include: Vec<String>,

        #[arg(
            long = "exclude",
            help = "Exclude node id patterns (wildcard by default, or re:<regex>)",
            action = clap::ArgAction::Append
        )]
        exclude: Vec<String>,

        #[arg(long, help = "Keep only nodes/edges within the root spec")]
        same_spec_only: bool,

        #[arg(
            long,
            default_value = "json",
            help = "Graph output format: json, markdown, mermaid, dot"
        )]
        graph_format: GraphOutputFormat,
    },

    /// Query dedicated WebIDL definitions
    #[command(long_about = "Query structured WebIDL definitions.\n\n\
        Supports exact anchors and canonical names:\n  \
        webspec-index idl HTML#dom-window-navigation\n  \
        webspec-index idl Window.navigation\n  \
        webspec-index idl Window.open()\n\n\
        Use --spec to narrow to one specification.")]
    Idl {
        /// Query string: SPEC#anchor, full URL, or canonical IDL name
        query: String,

        #[arg(long, short, help = "Limit lookup to a specific spec (e.g. HTML, DOM)")]
        spec: Option<String>,

        #[arg(long, short, default_value = "20", help = "Maximum number of matches")]
        limit: usize,
    },

    /// Update specifications to latest versions
    #[command(long_about = "Update indexed specifications to latest versions.\n\n\
        Without --spec, updates all currently indexed specs. Uses a 24h\n\
        freshness window unless --force is given.\n\n\
        Examples:\n  \
        webspec-index update\n  \
        webspec-index update --spec HTML\n  \
        webspec-index update --force")]
    Update {
        #[arg(long, short, help = "Update only this spec")]
        spec: Option<String>,

        #[arg(long, short, help = "Force update even if recently checked")]
        force: bool,
    },

    /// Clear the local database (remove all indexed data)
    ClearDb {
        #[arg(long, short, help = "Skip confirmation prompt")]
        yes: bool,
    },

    /// Analyze source files for spec references and step comment validation
    #[command(
        long_about = "Analyze source files for spec URL references and step comments.\n\n\
        Scans files for spec URLs (e.g. https://html.spec.whatwg.org/#navigate),\n\
        validates step comments against spec algorithms using fuzzy matching,\n\
        and reports coverage metrics.\n\n\
        Uses indentation-based scoping to correctly associate step comments\n\
        with their enclosing spec algorithm.\n\n\
        Examples:\n  \
        webspec-index analyze src/dom/base/Element.cpp\n  \
        webspec-index analyze src/ --recursive\n  \
        webspec-index analyze src/foo.cpp --threshold 0.9"
    )]
    Analyze {
        /// File or directory to analyze
        path: std::path::PathBuf,

        #[arg(long, short, help = "Recursively analyze directories")]
        recursive: bool,

        #[arg(
            long,
            short,
            default_value = "0.85",
            help = "Fuzzy match threshold (0.0-1.0)"
        )]
        threshold: f64,

        #[arg(
            long,
            default_value = "json",
            help = "Output format: json (human-readable) or searchfox (analysis records)"
        )]
        output_format: AnalyzeFormat,

        #[arg(
            long,
            help = "Write searchfox records to per-file analysis files in this directory \
                    (appending to existing files). Mirrors source tree structure. \
                    Requires --output-format=searchfox"
        )]
        output_dir: Option<std::path::PathBuf>,

        #[arg(
            long,
            help = "Strip this prefix from file paths when computing output paths \
                    (used with --output-dir to map source paths to analysis paths)"
        )]
        strip_prefix: Option<std::path::PathBuf>,
    },

    /// List indexed/discovered spec names and base URLs
    Specs,

    /// Start the Language Server Protocol server (stdio)
    Lsp,

    /// Update the local W3C spec list from csswg-drafts and w3c/groups
    ///
    /// Clones (or updates) csswg-drafts and w3c/groups, then regenerates
    /// data/w3c_specs.json. After running this command, rebuild to apply changes.
    #[command(long_about = "Update the local W3C spec list.\n\n\
        Clones (or updates) the csswg-drafts and w3c/groups repositories,\n\
        then regenerates data/w3c_specs.json with all discovered specs.\n\
        Rebuild after running this to apply the new spec list.\n\n\
        Examples:\n  \
        webspec-index update-spec-list\n  \
        webspec-index update-spec-list --csswg-dir /path/to/csswg-drafts")]
    UpdateSpecList {
        #[arg(
            long,
            default_value = "csswg-drafts",
            help = "Path to csswg-drafts clone"
        )]
        csswg_dir: std::path::PathBuf,

        #[arg(long, default_value = "groups", help = "Path to w3c/groups clone")]
        groups_dir: std::path::PathBuf,

        #[arg(
            long,
            default_value = "data/w3c_specs.json",
            help = "Output path for the spec list"
        )]
        output: std::path::PathBuf,
    },
}

fn is_llm_environment() -> bool {
    let has = |k| std::env::var(k).is_ok_and(|v| !v.is_empty());
    has("CLAUDECODE") || has("CODEX_SANDBOX") || has("GEMINI_CLI") || has("OPENCODE")
}

fn print_llm_help() {
    print!(
        r#"webspec-index: Query WHATWG/W3C/TC39 web specifications
query <SPEC#anchor|URL> [--format json|markdown]
search <Q> [-s SPEC] [-l N(20)] [--format json|markdown]
exists <SPEC#anchor|URL> exit:0=found,1=not
anchors <GLOB> [-s SPEC] [-l N(50)]
list <SPEC>
refs <SPEC#anchor> [-d incoming|outgoing|both(default)]
update [-s SPEC] [-f force]
clear-db [-y skip confirm]
specs — list indexed/discovered spec names+URLs
lsp — start LSP server on stdio
find-references <TARGET> [-d incoming|outgoing|both(default incoming)] [-l N(10)]
graph <SPEC#anchor|URL> [-d incoming|outgoing|both(default outgoing)] [--max-depth N(2)] [--max-nodes N(150)] [--include PATTERN --exclude PATTERN --same-spec-only] [--graph-format json|markdown|mermaid|dot]
idl <Q|SPEC#anchor|URL> [-s SPEC] [-l N(20)] [--format json|markdown]
SPEC#anchor examples: HTML#navigate, DOM#concept-tree, CSS-GRID#grid-container
Full URL also works: https://html.spec.whatwg.org/#navigate
Ex: query HTML#navigate|search "tree order" -s DOM|anchors "*-tree" -s DOM
Ex: refs HTML#navigate -d incoming|find-references Window.navigation|graph HTML#navigate --graph-format mermaid
Ex: idl Window.navigation|idl Window.open()|idl HTML#dom-window-navigation
"#
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    // Handle SIGPIPE gracefully (prevents broken pipe panics when piped through head/less)
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let version_checker = VersionChecker::new("webspec-index", env!("CARGO_PKG_VERSION"));
    version_checker.check_async();

    // Intercept --version for sync version warning
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("webspec-index {}", env!("CARGO_PKG_VERSION"));
        version_checker.print_warning_sync();
        return ExitCode::SUCCESS;
    }

    // LLM-friendly condensed help
    if is_llm_environment() && std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        print_llm_help();
        version_checker.print_warning();
        return ExitCode::SUCCESS;
    }

    let cli = Cli::parse();

    let result = run(cli).await;

    version_checker.print_warning();

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Query { spec_anchor } => {
            let result = webspec_index::query_section(&spec_anchor).await?;
            print_output(&cli.format, &result, format::query);
            Ok(ExitCode::SUCCESS)
        }

        Command::Search { query, spec, limit } => {
            let result = webspec_index::search_sections(&query, spec.as_deref(), limit)?;
            print_output(&cli.format, &result, format::search);
            Ok(ExitCode::SUCCESS)
        }

        Command::Exists { spec_anchor } => {
            let result = webspec_index::check_exists(&spec_anchor).await?;
            let found = result.exists;
            print_output(&cli.format, &result, format::exists);
            Ok(if found {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }

        Command::Anchors {
            pattern,
            spec,
            limit,
        } => {
            let result = webspec_index::find_anchors(&pattern, spec.as_deref(), limit)?;
            print_output(&cli.format, &result, format::anchors);
            Ok(ExitCode::SUCCESS)
        }

        Command::List { spec } => {
            let result = webspec_index::list_headings(&spec).await?;
            match cli.format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                OutputFormat::Markdown => {
                    print!("{}", format::list(&result));
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Refs {
            spec_anchor,
            direction,
        } => {
            let result = webspec_index::get_references(&spec_anchor, &direction).await?;
            print_output(&cli.format, &result, format::refs);
            Ok(ExitCode::SUCCESS)
        }

        Command::FindReferences {
            target,
            direction,
            limit,
        } => {
            let result = webspec_index::find_references(&target, &direction, limit).await?;
            print_output(&cli.format, &result, format::find_references);
            Ok(ExitCode::SUCCESS)
        }

        Command::Graph {
            spec_anchor,
            direction,
            max_depth,
            max_nodes,
            include,
            exclude,
            same_spec_only,
            graph_format,
        } => {
            let result = webspec_index::graph_section(
                &spec_anchor,
                &direction,
                max_depth,
                max_nodes,
                &include,
                &exclude,
                same_spec_only,
            )
            .await?;
            match graph_format {
                GraphOutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                GraphOutputFormat::Markdown => {
                    print!("{}", format::graph(&result));
                }
                GraphOutputFormat::Mermaid => {
                    print!("{}", format::graph_mermaid(&result));
                }
                GraphOutputFormat::Dot => {
                    print!("{}", format::graph_dot(&result));
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Idl { query, spec, limit } => {
            let result = webspec_index::query_idl(&query, spec.as_deref(), limit).await?;
            print_output(&cli.format, &result, format::idl);
            Ok(ExitCode::SUCCESS)
        }

        Command::Update { spec, force } => {
            let results = webspec_index::update_specs(spec.as_deref(), force).await?;
            let output: Vec<model::UpdateEntry> = results
                .into_iter()
                .map(|(name, snapshot_id)| model::UpdateEntry {
                    spec: name,
                    updated: snapshot_id.is_some(),
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(ExitCode::SUCCESS)
        }

        Command::ClearDb { yes } => {
            if !yes {
                eprint!("This will delete all indexed data. Continue? [y/N] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    return Ok(ExitCode::SUCCESS);
                }
            }
            let path = webspec_index::clear_database()?;
            eprintln!("Deleted {path}");
            Ok(ExitCode::SUCCESS)
        }

        Command::Analyze {
            path,
            recursive,
            threshold,
            output_format,
            output_dir,
            strip_prefix,
        } => {
            run_analyze(
                &path,
                recursive,
                threshold,
                &output_format,
                output_dir.as_deref(),
                strip_prefix.as_deref(),
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Specs => {
            let urls = webspec_index::spec_urls();
            println!("{}", serde_json::to_string_pretty(&urls)?);
            Ok(ExitCode::SUCCESS)
        }

        Command::Lsp => {
            webspec_index::lsp::serve_stdio().await;
            Ok(ExitCode::SUCCESS)
        }

        Command::UpdateSpecList {
            csswg_dir,
            groups_dir,
            output,
        } => {
            let (csswg_count, standalone_count, entries) =
                webspec_index::spec_list::update(&csswg_dir, &groups_dir, &output)?;
            let conn = webspec_index::db::open_or_create_db()?;
            for e in &entries {
                webspec_index::db::write::seed_spec(&conn, &e.name, &e.base_url, &e.provider)?;
            }
            eprintln!(
                "wrote {} specs to {} ({} CSSWG + {} standalone); seeded DB",
                csswg_count + standalone_count,
                output.display(),
                csswg_count,
                standalone_count
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// DB-backed spec resolver for the analyze command.
///
/// Uses `DashMap` for thread-safe caching (safe for future parallelization).
struct DbResolver {
    cache: dashmap::DashMap<String, Option<String>>,
}

impl DbResolver {
    fn new() -> Self {
        DbResolver {
            cache: dashmap::DashMap::new(),
        }
    }

    /// Return all successfully resolved sections as a map of
    /// "SPEC_<spec>_<anchor>" -> content (the same symbol names used
    /// in searchfox analysis records).
    fn resolved_sections(&self) -> std::collections::HashMap<String, String> {
        self.cache
            .iter()
            .filter_map(|entry| {
                let content = entry.value().as_ref()?;
                let (spec, anchor) = entry.key().split_once('#')?;
                let sym = format!("SPEC_{spec}_{anchor}");
                Some((sym, content.clone()))
            })
            .collect()
    }
}

impl webspec_index::analyze::file::SpecResolver for DbResolver {
    fn resolve(&self, spec: &str, anchor: &str) -> Option<String> {
        let key = format!("{spec}#{anchor}");
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(webspec_index::query_section(&key))
                .ok()
        });
        let content = result.and_then(|r| r.content).filter(|c| !c.is_empty());
        self.cache.insert(key, content.clone());
        content
    }
}

/// Source file extensions to scan when analyzing directories.
const SOURCE_EXTENSIONS: &[&str] = &[
    "cpp", "cc", "cxx", "c", "h", "hpp", "hxx", "rs", "js", "mjs", "jsm", "py", "java",
];

fn is_source_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

/// Collect source files to analyze.
fn collect_files(
    path: &std::path::Path,
    recursive: bool,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        anyhow::bail!("{} is not a file or directory", path.display());
    }
    let mut files = Vec::new();
    let mut dirs = vec![path.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            if ft.is_file() && is_source_file(&entry.path()) {
                files.push(entry.path());
            } else if ft.is_dir() && recursive {
                dirs.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Run the analyze subcommand.
async fn run_analyze(
    path: &std::path::Path,
    recursive: bool,
    threshold: f64,
    format: &AnalyzeFormat,
    output_dir: Option<&std::path::Path>,
    strip_prefix: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    use webspec_index::analyze::file::{analyze_file, FileAnalysisView};
    use webspec_index::analyze::scanner::SpecUrl;
    use webspec_index::analyze::searchfox::to_searchfox_records;

    if output_dir.is_some() && !matches!(format, AnalyzeFormat::Searchfox) {
        anyhow::bail!("--output-dir requires --output-format=searchfox");
    }

    let files = collect_files(path, recursive)?;
    if files.is_empty() {
        eprintln!("No source files found in {}", path.display());
        return Ok(());
    }

    // Build spec URL list from the database.
    let spec_urls: Vec<SpecUrl> = webspec_index::spec_urls()
        .into_iter()
        .map(|e| SpecUrl {
            spec: e.spec,
            base_url: e.base_url,
        })
        .collect();

    let resolver = DbResolver::new();
    let mut files_with_refs = 0;

    for file_path in &files {
        let text = match std::fs::read_to_string(file_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("warning: {}: {e}", file_path.display());
                continue;
            }
        };

        let result = analyze_file(&text, &spec_urls, &resolver, threshold);
        if result.scopes.is_empty() {
            continue;
        }

        let view = FileAnalysisView::from(&result);

        match format {
            AnalyzeFormat::Json => {
                let output = serde_json::json!({
                    "file": file_path.to_string_lossy(),
                    "scopes": view.scopes,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            AnalyzeFormat::Searchfox => {
                let records = to_searchfox_records(&view);
                if records.is_empty() {
                    continue;
                }
                if let Some(out_dir) = output_dir {
                    let relative = if let Some(prefix) = strip_prefix {
                        file_path.strip_prefix(prefix).unwrap_or(file_path)
                    } else {
                        file_path.as_path()
                    };
                    let analysis_path = out_dir.join(relative);
                    if let Some(parent) = analysis_path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    use std::io::Write;
                    let mut f = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&analysis_path)?;
                    writeln!(f, "{records}")?;
                } else {
                    println!("{records}");
                }
            }
        }
        files_with_refs += 1;
    }

    if let Some(out_dir) = output_dir {
        let sections = resolver.resolved_sections();
        if !sections.is_empty() {
            let sections_path = out_dir.join("spec-sections.json");
            let json = serde_json::to_string(&sections)?;
            std::fs::write(&sections_path, json)?;
            eprintln!(
                "spec-analyze: wrote {} spec sections to {}",
                sections.len(),
                sections_path.display()
            );
        }
    }

    eprintln!("spec-analyze: {files_with_refs} files with spec references");
    Ok(())
}

/// Print output in the requested format
fn print_output<T: serde::Serialize>(
    fmt: &OutputFormat,
    value: &T,
    markdown_fn: impl FnOnce(&T) -> String,
) {
    match fmt {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(value).expect("serialization failed")
            );
        }
        OutputFormat::Markdown => {
            print!("{}", markdown_fn(value));
        }
    }
}
