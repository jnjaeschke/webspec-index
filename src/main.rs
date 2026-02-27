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

    /// Update specifications to latest versions
    #[command(
        long_about = "Update specifications to latest versions from WHATWG/W3C/TC39.\n\n\
        Without --spec, updates all known specs. Uses 24h cache to avoid\n\
        redundant fetches unless --force is given.\n\n\
        Examples:\n  \
        webspec-index update\n  \
        webspec-index update --spec HTML\n  \
        webspec-index update --force"
    )]
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

    /// List all known spec names and base URLs
    Specs,

    /// Start the Language Server Protocol server (stdio)
    Lsp,
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
specs — list all known spec names+URLs
lsp — start LSP server on stdio
SPEC#anchor examples: HTML#navigate, DOM#concept-tree, CSS-GRID#grid-container
Full URL also works: https://html.spec.whatwg.org/#navigate
Ex: query HTML#navigate|search "tree order" -s DOM|anchors "*-tree" -s DOM
Ex: refs HTML#navigate -d incoming|list DOM|update -s HTML
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

        Command::Specs => {
            let urls = webspec_index::spec_urls();
            println!("{}", serde_json::to_string_pretty(&urls)?);
            Ok(ExitCode::SUCCESS)
        }

        Command::Lsp => {
            webspec_index::lsp::serve_stdio().await;
            Ok(ExitCode::SUCCESS)
        }
    }
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
