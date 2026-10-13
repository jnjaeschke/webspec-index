use std::process::ExitCode;

use anyhow::{anyhow, Context};
use clap::{Args, Parser, Subcommand, ValueEnum};
use moz_cli_version_check::VersionChecker;

use webspec_index::state::slice::{
    apply_view, validate_shape, FeedingSelector, SliceError, SliceErrorCode, ViewRequest,
};
use webspec_index::{content_filter, format, model};

#[derive(Parser, Debug)]
#[command(
    name = "webspec-index",
    version,
    about = "Query WHATWG/W3C/TC39/IETF web specifications",
    long_about = "A command-line tool for querying web specification sections, algorithms, \
        and cross-references.\n\n\
        Indexes specs from WHATWG (HTML, DOM, URL, Fetch, …), W3C (CSS, Geometry, …), \
        TC39 (ECMAScript), and IETF (RFCs and Internet Drafts). Specs are fetched and \
        cached locally on first use.\n\n\
        IETF specs are resolved dynamically — use the RFC number or draft name:\n  \
        webspec-index query RFC9110#section-5\n  \
        webspec-index query draft-touch-sne#section-1\n  \
        webspec-index query draft-touch-sne-02#section-1   (pinned version)\n\n\
        PR previews (WHATWG specs, TC39 proposals) — query sections as modified by an open PR:\n  \
        webspec-index query HTML#navigate --pr 12345\n  \
        webspec-index query HTML --pr 12345 --diff\n  \
        webspec-index query proposal-defer-import-eval --pr 85 --diff\n  \
        webspec-index clear-pr                              (list cached PRs)\n  \
        webspec-index clear-pr --spec HTML --pr 12345       (remove cached PR)\n\n\
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

#[derive(ValueEnum, Clone, Debug)]
enum FlowOutputFormat {
    Json,
    Mermaid,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
enum LinksArg {
    /// Rewrite known-spec URLs to SPEC#anchor (e.g. HTML#navigate); keeps others as full URLs
    #[default]
    Short,
    /// Keep full absolute URLs
    Full,
    /// Emit link text only; remove all bracket and URL markup
    None,
}

impl From<LinksArg> for content_filter::LinksMode {
    fn from(v: LinksArg) -> Self {
        match v {
            LinksArg::Short => Self::Short,
            LinksArg::Full => Self::Full,
            LinksArg::None => Self::None,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum EffectsModeArg {
    Auto,
    Cached,
    Off,
}

impl From<EffectsModeArg> for webspec_index::effects::EffectsMode {
    fn from(value: EffectsModeArg) -> Self {
        match value {
            EffectsModeArg::Auto => Self::Auto,
            EffectsModeArg::Cached => Self::Cached,
            EffectsModeArg::Off => Self::Off,
        }
    }
}

#[derive(Args, Debug)]
struct QueryEffectsArgs {
    #[arg(
        long,
        value_enum,
        help = "Possible-effects analysis: auto (default; off in views), cached, or off"
    )]
    effects: Option<EffectsModeArg>,

    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append, help = "Add a semantic rule package directory")]
    rules: Vec<String>,

    #[arg(long, default_value = "web", help = "Semantic analysis environment")]
    environment: String,
}

#[derive(Args, Debug)]
struct UpdateEffectsArgs {
    #[arg(
        long,
        value_enum,
        default_value = "auto",
        help = "Effects refresh: auto or off"
    )]
    effects: UpdateEffectsMode,

    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append, help = "Add a semantic rule package directory")]
    rules: Vec<String>,

    #[arg(long, default_value = "web", help = "Semantic analysis environment")]
    environment: String,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateEffectsMode {
    Auto,
    Off,
}

#[derive(Args, Debug)]
struct EffectsArgs {
    /// Section identifier: SPEC#anchor or full URL
    #[arg(required_unless_present = "all", conflicts_with = "all")]
    subject: Option<String>,

    #[arg(
        long,
        conflicts_with = "subject",
        help = "Build the effects graph for the indexed corpus"
    )]
    all: bool,

    #[arg(
        long,
        help = "Build effects from scratch instead of incrementally (with --all)"
    )]
    rebuild: bool,

    #[arg(long, help = "Return summaries without witness explanations")]
    summary_only: bool,

    #[arg(long, conflicts_with_all = ["summary_only", "all"], help = "Show query's short effects preview: up to 12 effects, without witness traces")]
    compact: bool,

    #[arg(long, conflicts_with_all = ["step_id", "body_id"], help = "Dotted step number, for example 14.12.3")]
    step: Option<String>,

    #[arg(long, conflicts_with_all = ["step", "body_id"], help = "Exact structural step ID")]
    step_id: Option<String>,

    #[arg(long, conflicts_with_all = ["step", "step_id"], help = "Exact structural body ID")]
    body_id: Option<String>,

    #[arg(long, help = "Select one effect kind")]
    kind: Option<String>,

    #[arg(long, help = "Select one effect category")]
    category: Option<String>,

    #[arg(long = "rule", help = "Select effects supported by this rule")]
    rule_id: Option<String>,

    #[arg(long, help = "Select one effect handle")]
    effect_id: Option<String>,

    #[arg(long, help = "Restrict witnesses to one source occurrence")]
    occurrence_id: Option<String>,

    #[arg(long, value_name = "PATH", action = clap::ArgAction::Append, help = "Add a semantic rule package directory")]
    rules: Vec<String>,

    #[arg(long, default_value = "web", help = "Semantic analysis environment")]
    environment: String,

    #[arg(long, default_value_t = webspec_index::effects::DEFAULT_MAX_BODIES, value_parser = clap::value_parser!(u64).range(1..))]
    max_bodies: u64,

    #[arg(long, default_value_t = webspec_index::effects::DEFAULT_MAX_RELATIONSHIPS, value_parser = clap::value_parser!(u64).range(1..))]
    max_relationships: u64,

    #[arg(long, default_value_t = webspec_index::effects::DEFAULT_MAX_STATES, value_parser = clap::value_parser!(u64).range(1..))]
    max_states: u64,

    #[arg(long, default_value_t = webspec_index::effects::DEFAULT_MAX_DEPTH, value_parser = clap::value_parser!(u64).range(1..))]
    max_depth: u64,

    #[arg(long, default_value_t = webspec_index::effects::DEFAULT_MAX_WITNESS_STATES, value_parser = clap::value_parser!(u64).range(1..))]
    max_witness_states: u64,

    #[arg(long, short, default_value_t = webspec_index::effects::DEFAULT_WITNESS_LIMIT, value_parser = clap::value_parser!(u64).range(1..), help = "Maximum witnesses per effect group")]
    limit: u64,
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
        webspec-index query \"https://html.spec.whatwg.org/#navigate\"\n\n\
        Link rendering (--links):\n  \
        short (default) — rewrite known-spec URLs to SPEC#anchor (e.g. HTML#navigate)\n  \
        full            — keep absolute URLs\n  \
        none            — emit link text only, no URL or bracket markup\n\n\
        Use --no-notes to drop Note / Example / Warning / Issue advisement blocks.\n\n\
        Views of one algorithm (the spec's wording, whole steps dropped, every omission marked):\n  \
        --involving VAR[,VAR]   steps that use the variables and variables derived from them\n  \
        --feeding STEP[:VAR]    earlier steps that define the variables step STEP uses\n  \
        --steps PATH[,PATH]     these steps with their substeps\n  \
        --depth N               outline: steps down to depth N (combines with the others)\n  \
        webspec-index query HTML#navigate --involving historyHandling\n  \
        webspec-index query HTML#navigate --feeding 24.9.1\n  \
        webspec-index query HTML#navigate --depth 1\n\n\
        Use --pr to query against a PR preview — WHATWG specs (via whatpr.org) or\n\
        TC39 proposals (via the PR's built index.html). Sections not modified by\n\
        the PR fall back to the merge base.\n\
        Use --diff to see a section-level diff of the whole PR vs its merge base;\n\
        with --diff the #anchor is optional (a bare spec name previews all changes):\n  \
        webspec-index query HTML#navigate --pr 12345\n  \
        webspec-index query HTML --pr 12345 --diff --format markdown\n  \
        webspec-index query proposal-defer-import-eval --pr 85 --diff")]
    Query {
        /// Section identifier: SPEC#anchor or full URL (bare SPEC allowed with --diff)
        spec_anchor: String,

        #[arg(long, help = "Query against a WHATWG or TC39 proposal PR preview")]
        pr: Option<i64>,

        #[arg(long, help = "Show diff between PR and merge base (requires --pr)")]
        diff: bool,

        #[arg(long, help = "Force re-fetch of PR preview data")]
        force_update: bool,

        #[arg(
            long,
            value_enum,
            default_value = "short",
            help = "Link rendering: short (SPEC#anchor), full (absolute URLs), or none (text only)"
        )]
        links: LinksArg,

        #[arg(
            long,
            help = "Strip Note / Example / Warning / Issue advisement blocks from content"
        )]
        no_notes: bool,

        #[arg(long, value_delimiter = ',', value_name = "VAR", conflicts_with_all = ["feeding", "steps", "diff"],
              help = "View: steps involving these variables and the variables derived from them")]
        involving: Vec<String>,

        #[arg(long, value_name = "STEP[:VAR,…]", conflicts_with_all = ["involving", "steps", "diff"],
              help = "View: earlier steps that feed the variables of STEP")]
        feeding: Option<String>,

        #[arg(long, value_delimiter = ',', value_name = "PATH", conflicts_with_all = ["involving", "feeding", "diff"],
              help = "View: these steps with their substeps")]
        steps: Vec<String>,

        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..), conflicts_with = "diff",
              help = "View: outline to this step depth")]
        depth: Option<u32>,

        #[command(flatten)]
        effect_options: QueryEffectsArgs,
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
        limit: u32,

        #[arg(
            long,
            help = "Search within a WHATWG or TC39 proposal PR preview (requires --spec)"
        )]
        pr: Option<i64>,

        #[arg(long, help = "Force re-fetch of PR preview data")]
        force_update: bool,
    },

    /// Check if a section exists (exit code 0 = found, 1 = not found)
    Exists {
        /// Section identifier: SPEC#anchor or full URL
        spec_anchor: String,

        #[arg(long, help = "Query against a WHATWG or TC39 proposal PR preview")]
        pr: Option<i64>,

        #[arg(long, help = "Force re-fetch of PR preview data")]
        force_update: bool,
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
        limit: u32,

        #[arg(
            long,
            help = "Search within a WHATWG or TC39 proposal PR preview (requires --spec)"
        )]
        pr: Option<i64>,

        #[arg(long, help = "Force re-fetch of PR preview data")]
        force_update: bool,
    },

    /// List all headings in a specification
    List {
        /// Spec name (e.g. HTML, DOM, CSS-GRID)
        spec: String,

        #[arg(long, help = "Query against a WHATWG or TC39 proposal PR preview")]
        pr: Option<i64>,

        #[arg(long, help = "Force re-fetch of PR preview data")]
        force_update: bool,
    },

    /// Get cross-references for a section
    #[command(long_about = "Get cross-references for a section.\n\n\
        Shows which other spec sections reference this one (incoming)\n\
        and which sections this one references (outgoing).\n\n\
        Target can be SPEC#anchor (exact), full URL, or shorthand such as\n\
        Interface.member (heuristic match against indexed sections).\n\n\
        Examples:\n  \
        webspec-index refs HTML#navigate\n  \
        webspec-index refs HTML#navigate --direction incoming\n  \
        webspec-index refs Window.navigation --limit 5")]
    Refs {
        /// Target: SPEC#anchor, full URL, or shorthand (e.g. Window.navigation)
        target: String,

        #[arg(
            long,
            short,
            default_value = "both",
            help = "Reference direction: incoming, outgoing, or both"
        )]
        direction: String,

        #[arg(long, short, default_value = "10", help = "Maximum number of matches")]
        limit: u32,

        #[arg(long, help = "Query against a WHATWG or TC39 proposal PR preview")]
        pr: Option<i64>,

        #[arg(long, help = "Force re-fetch of PR preview data")]
        force_update: bool,

        #[arg(
            long,
            help = "Only references of this kind: step, note, idl, prose. \
                    Use --kind step for algorithm calls without prose mentions"
        )]
        kind: Option<String>,
    },

    /// Trace the algorithm call chain from one section to another
    #[command(
        long_about = "Trace routes through the reference graph from one section to another.\n\n\
        Both endpoints must be exact (SPEC#anchor or full URL). Each hop reports the\n\
        step that makes the call, the step's text, and any enclosing guard steps, so\n\
        the markdown output is a ready-made trace.\n\n\
        Defaults to --kind step, i.e. algorithm calls only. Pass --kind any to include\n\
        prose mentions, IDL blocks and notes.\n\n\
        --detail picks how much of each hop to show:\n  \
        verbose (default) — step text, guards and call-site link, ready to paste\n  \
        edges             — SPEC#anchor plus step number, no prose and no URLs\n  \
        compact           — one linked line per hop, an edge's repeated call sites\n                      \
        collapsed onto it\n\n\
        Reduced levels are for comparing route shapes, not for judging one: the\n\
        guards they drop are what decide whether a route is taken.\n\n\
        Examples:\n  \
        webspec-index trace HTML#dom-location-assign HTML#event-navigateerror\n  \
        webspec-index trace HTML#navigate DOM#concept-tree --max-depth 4 --format markdown\n  \
        webspec-index trace HTML#dom-location-assign HTML#event-navigateerror --detail compact"
    )]
    Trace {
        /// Starting section: SPEC#anchor or full URL
        from: String,

        /// Target section: SPEC#anchor or full URL
        to: String,

        #[arg(long, default_value = "6", help = "Maximum number of hops")]
        max_depth: usize,

        #[arg(
            long,
            default_value = "step",
            help = "Reference kind to traverse: step, note, idl, prose, or any"
        )]
        kind: String,

        #[arg(long, short, default_value = "20", help = "Maximum number of traces")]
        limit: usize,

        #[arg(
            long,
            short,
            default_value = "verbose",
            help = "How much of each hop to show: verbose, edges, or compact"
        )]
        detail: String,
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

    /// Extract the control-flow graph of an algorithm section
    #[command(
        long_about = "Extract the control-flow graph of an algorithm section.\n\n\
        Parses the stored markdown steps and outgoing refs to produce a graph of\n\
        nodes (steps, branches, loops, terminals) and edges (next, then, else, loop,\n\
        jump, call).  Use --flow-format mermaid to get a Mermaid flowchart you can\n\
        paste into documentation or a diagram tool.\n\n\
        Examples:\n  \
        webspec-index flow HTML#navigate\n  \
        webspec-index flow HTML#navigate --flow-format mermaid"
    )]
    Flow {
        /// Section identifier: SPEC#anchor or full URL
        spec_anchor: String,

        #[arg(
            long,
            default_value = "json",
            help = "Flow output format: json or mermaid"
        )]
        flow_format: FlowOutputFormat,
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
        limit: u32,

        #[arg(long, help = "Query against a WHATWG or TC39 proposal PR preview")]
        pr: Option<i64>,

        #[arg(long, help = "Force re-fetch of PR preview data")]
        force_update: bool,
    },

    /// Show which steps write a field, the fields of a type, or an algorithm's signature and calls
    #[command(
        long_about = "Show which algorithm steps and normative prose write a field,\n\
        list the fields of a type (with inherited fields), or show an algorithm's\n\
        signature, statements, calls (--calls) and callers (--callers).\n\n\
        Selectors:\n  \
        SPEC#anchor        field, set member, type or algorithm anchor (a spec URL works too)\n  \
        TYPE               IDL name or concept name, e.g. Document, navigable\n  \
        TYPE.FIELD         field by name through inheritance, e.g. \"Element.node document\"\n  \
        TYPE.GLOB          fields of TYPE by name or anchor, e.g. \"Document.*sandbox*\"\n  \
        SPEC#GLOB          fields of a spec, e.g. \"HTML#*sandbox*\"\n\n\
        Answers are may-semantics: a write site means the step may assign the field."
    )]
    State {
        /// Field, type, or glob selector
        #[arg(required_unless_present = "coverage")]
        selector: Option<String>,
        #[arg(long, help = "Hide the initializations group")]
        no_inits: bool,
        #[arg(
            long,
            help = "List unclassified occurrences instead of only counting them"
        )]
        unclassified: bool,
        #[arg(long, help = "Algorithm view: list each call with its bound arguments")]
        calls: bool,
        #[arg(long, help = "Algorithm view: list the algorithms that call this one")]
        callers: bool,
        #[arg(
            long,
            help = "Algorithm view: list the statements the model could not structure"
        )]
        opaque: bool,
        #[arg(
            long,
            short,
            help = "Sites per group (field view), fields (lists), rows per table (type view), or caller algorithms (algorithm view)"
        )]
        limit: Option<u32>,
        #[arg(long, value_name = "PATH", action = clap::ArgAction::Append, help = "Add a semantic rule package directory")]
        rules: Vec<String>,
        #[arg(
            long,
            value_name = "SPEC",
            conflicts_with = "selector",
            help = "Show coverage metrics for SPEC"
        )]
        coverage: Option<String>,
    },

    /// Update specifications to latest versions
    #[command(long_about = "Update indexed specifications to latest versions.\n\n\
        Without --spec, updates all currently indexed specs. Uses a 24h\n\
        freshness window unless --force is given.\n\n\
        --force re-parses every spec but reads from the on-disk HTML cache\n\
        when available, avoiding network round-trips for unchanged specs.\n\
        --refetch bypasses the HTML cache and re-downloads everything.\n\n\
        Examples:\n  \
        webspec-index update\n  \
        webspec-index update --spec HTML\n  \
        webspec-index update --force\n  \
        webspec-index update --force --refetch\n  \
        webspec-index update --providers whatwg,w3c,tc39")]
    Update {
        #[arg(long, short, help = "Update only this spec")]
        spec: Option<String>,

        #[arg(long, short, help = "Force update even if recently checked")]
        force: bool,

        #[arg(
            long,
            help = "Bypass the on-disk HTML cache and re-download all specs (requires --force)"
        )]
        refetch: bool,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Update only specs from these providers (comma-separated: whatwg,w3c,tc39)"
        )]
        providers: Vec<String>,

        #[command(flatten)]
        effect_options: UpdateEffectsArgs,
    },

    /// Re-parse indexed specs from the on-disk HTML cache without network access
    #[command(
        long_about = "Re-parse indexed specifications from the on-disk HTML cache.\n\n\
        Reads the cached HTML for each spec and re-runs the parser, writing\n\
        fresh sections, references, and IDL definitions to the database. No\n\
        network access is performed. Specs without a cached HTML file are\n\
        reported and skipped.\n\n\
        Use this after a parser change to rebuild the index without waiting\n\
        for a full re-download of all specs.\n\n\
        Examples:\n  \
        webspec-index reparse\n  \
        webspec-index reparse --spec HTML\n  \
        webspec-index reparse --providers whatwg,w3c"
    )]
    Reparse {
        #[arg(long, short, help = "Re-parse only this spec")]
        spec: Option<String>,

        #[arg(
            long,
            value_delimiter = ',',
            help = "Re-parse only specs from these providers (comma-separated: whatwg,w3c,tc39)"
        )]
        providers: Vec<String>,

        #[command(flatten)]
        effect_options: UpdateEffectsArgs,
    },

    /// Inspect possible effects of a specification algorithm or step
    #[command(
        long_about = "Inspect possible effects and their evidence for one subject, or build the effects graph with --all."
    )]
    Effects(Box<EffectsArgs>),

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

        #[arg(
            long,
            help = "Write the resolved spec-sections map to this path instead of inside \
                    --output-dir. Use this when --output-dir feeds a consumer (e.g. \
                    searchfox's analysis-file sweep) that expects every file in that \
                    directory to be a per-file record stream, since spec-sections.json \
                    is a single aggregate JSON object, not one of those. \
                    Requires --output-format=searchfox"
        )]
        sections_output: Option<std::path::PathBuf>,
    },

    /// List indexed/discovered spec names and base URLs
    Specs,

    /// Start the Language Server Protocol server (stdio)
    Lsp {
        #[arg(long, value_name = "PATH", action = clap::ArgAction::Append, help = "Add a semantic rule package directory")]
        rules: Vec<String>,

        #[arg(long, default_value = "web", help = "Semantic analysis environment")]
        environment: String,
    },

    /// Remove cached PR preview data
    #[command(long_about = "Remove cached PR preview data.\n\n\
        Without arguments, lists all cached PR snapshots.\n\
        With --spec and --pr, removes data for a specific PR.\n\
        With --spec alone, removes all PR data for that spec.\n\
        With --all, removes all cached PR data across all specs.\n\n\
        Each cached PR stores the rendered preview pages and a full merge base\n\
        snapshot, which can be large (7MB+ for the HTML spec). Use this command\n\
        to reclaim disk space.\n\n\
        Examples:\n  \
        webspec-index clear-pr\n  \
        webspec-index clear-pr --spec HTML --pr 12345\n  \
        webspec-index clear-pr --spec HTML\n  \
        webspec-index clear-pr --all")]
    ClearPr {
        #[arg(long, short, help = "Spec to clear PR data for")]
        spec: Option<String>,

        #[arg(long, help = "Specific PR number to clear")]
        pr: Option<i64>,

        #[arg(long, help = "Clear all cached PR data")]
        all: bool,
    },

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

    /// Write the chunked read-only database and manifest the web UI serves
    ExportWeb {
        #[arg(long, help = "Output directory (created if missing)")]
        out: std::path::PathBuf,
        #[arg(
            long,
            value_delimiter = ',',
            default_value = "whatwg,w3c,tc39",
            help = "Providers to include"
        )]
        providers: Vec<String>,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Export only these specs by name, comma-separated (e.g. html,dom,fetch); empty = all"
        )]
        specs: Vec<String>,
        #[arg(long, default_value = "52428800", help = "Chunk size in bytes")]
        chunk_size: u64,
        #[arg(
            long,
            default_value = "943718400",
            help = "Fail if the database exceeds this many bytes"
        )]
        max_size: u64,
    },
}

fn is_llm_environment() -> bool {
    let has = |k| std::env::var(k).is_ok_and(|v| !v.is_empty());
    has("CLAUDECODE") || has("CODEX_SANDBOX") || has("GEMINI_CLI") || has("OPENCODE")
}

fn print_llm_help() {
    print!(
        r#"webspec-index: Query WHATWG/W3C/TC39 web specifications
query <SPEC#anchor|URL> [--involving VAR|--feeding STEP[:VAR]|--steps PATH] [--depth N] [--links short(default)|full|none] [--no-notes] [--effects auto|cached|off] [--rules PATH] [--environment NAME] [--pr N] [--diff] [--format json|markdown]
search <Q> [-s SPEC] [-l N(20)] [--pr N (requires -s)] [--format json|markdown]
exists <SPEC#anchor|URL> [--pr N] exit:0=found,1=not
anchors <GLOB> [-s SPEC] [-l N(50)] [--pr N (requires -s)]
list <SPEC> [--pr N]
refs <SPEC#anchor|TARGET> [-d incoming|outgoing|both(default)] [-l N(10)] [--pr N] [--kind step|note|idl|prose]
trace <FROM> <TO> [--max-depth N(6)] [--kind step(default)|note|idl|prose|any] [-l N(20)] [-d verbose(default)|edges|compact] [--format json|markdown]
effects [<SPEC#anchor|URL> | --all] [--step N.N|--step-id ID|--body-id ID] [--compact|--summary-only] [--kind KIND] [--category CATEGORY] [--rule ID] [--effect-id ID] [--occurrence-id ID] [--rules PATH] [--environment NAME]
update [-s SPEC] [-f force] [--refetch] [--providers a,b] [--effects auto|off] [--rules PATH] [--environment NAME]
reparse [-s SPEC] [--providers a,b] — re-parse from on-disk HTML cache, no network
clear-db [-y skip confirm]
clear-pr [--all | -s SPEC [--pr N]] — list or remove cached PR data
export-web --out DIR [--providers a,b] [--specs HTML,DOM] [--chunk-size N] [--max-size N] — write chunked read-only DB for the web UI
specs — list indexed/discovered spec names+URLs
lsp [--rules PATH] [--environment NAME] — start LSP server on stdio
graph <SPEC#anchor|URL> [-d incoming|outgoing|both(default outgoing)] [--max-depth N(2)] [--max-nodes N(150)] [--include PATTERN --exclude PATTERN --same-spec-only] [--graph-format json|markdown|mermaid|dot]
flow <SPEC#anchor|URL> [--flow-format json|mermaid]
idl <Q|SPEC#anchor|URL> [-s SPEC] [-l N(20)] [--pr N] [--format json|markdown]
state <SELECTOR> | --coverage SPEC [--no-inits] [--unclassified] [--calls] [--callers] [--opaque] [-l N] [--rules PATH] [--format json|markdown]
SPEC#anchor examples: HTML#navigate, DOM#concept-tree, CSS-GRID#grid-container
Full URL also works: https://html.spec.whatwg.org/#navigate
--pr N: query against a PR preview (WHATWG specs or TC39 proposals); --diff: show diff vs merge base (requires --pr; #anchor optional with --diff)
Ex: query HTML#navigate|search "tree order" -s DOM|anchors "*-tree" -s DOM
Ex: query HTML#navigate --involving historyHandling|query HTML#navigate --feeding 24.9.1|query HTML#navigate --depth 1
Ex: refs HTML#navigate -d incoming|refs Window.navigation|graph HTML#navigate --graph-format mermaid
Ex: trace HTML#dom-location-assign HTML#event-navigateerror --format markdown|trace A B -d compact
Ex: idl Window.navigation|idl Window.open()|idl HTML#dom-window-navigation
Ex: state HTML#is-initial-about:blank|state "Element.node document"|state Document|state HTML#navigate --calls
Ex: query HTML#navigate --pr 1234|query HTML --pr 1234 --diff|query proposal-defer-import-eval --pr 85 --diff
"#
    );
}

/// The view the query flags select, normalized and checked so that selector errors surface before
/// any lookup; `None` without view flags.
fn view_request(
    involving: Vec<String>,
    feeding: Option<String>,
    steps: Vec<String>,
    depth: Option<u32>,
) -> anyhow::Result<Option<ViewRequest>> {
    if involving.is_empty() && feeding.is_none() && steps.is_empty() && depth.is_none() {
        return Ok(None);
    }
    let feeding = feeding
        .as_deref()
        .map(FeedingSelector::parse)
        .transpose()
        .map_err(|e| anyhow!("{e}"))?;
    let view = ViewRequest {
        involving,
        feeding,
        steps,
        depth,
    };
    validate_shape(&view).map(Some).map_err(|e| anyhow!("{e}"))
}

/// Views print the spec's wording, so effects are off in them unless asked for.
fn effects_mode(
    arg: Option<EffectsModeArg>,
    has_view: bool,
) -> webspec_index::effects::EffectsMode {
    match arg {
        Some(mode) => mode.into(),
        None if has_view => webspec_index::effects::EffectsMode::Off,
        None => webspec_index::effects::EffectsMode::Auto,
    }
}

impl Command {
    /// `lsp` and `analyze` block worker threads with `block_in_place`, and
    /// `update`/`reparse` fetch and parse specs concurrently. Lookups run one
    /// query and exit, so they skip spawning a worker per core.
    fn needs_multi_thread_runtime(&self) -> bool {
        matches!(
            self,
            Command::Lsp { .. }
                | Command::Analyze { .. }
                | Command::Update { .. }
                | Command::Reparse { .. }
        )
    }
}

fn main() -> ExitCode {
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

    let mut runtime = if cli.command.needs_multi_thread_runtime() {
        tokio::runtime::Builder::new_multi_thread()
    } else {
        tokio::runtime::Builder::new_current_thread()
    };
    let result = runtime
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(run(cli)));

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
        Command::Query {
            spec_anchor,
            pr,
            diff,
            force_update,
            links,
            no_notes,
            involving,
            feeding,
            steps,
            depth,
            effect_options,
        } => {
            let view = view_request(involving, feeding, steps, depth)?;
            if view.is_some() && pr.is_some() {
                return Err(anyhow!(
                    "{}",
                    SliceError {
                        code: SliceErrorCode::Unavailable,
                        message:
                            "views need the indexed trunk snapshot; PR previews have no slice index"
                                .into(),
                        candidates: vec![],
                    }
                ));
            }
            let pr_opts = pr.map(|n| model::PrOpts {
                pr_number: n,
                force_update,
            });
            if diff {
                let opts = pr_opts.as_ref().context("--diff requires --pr")?;
                // A whole-PR diff needs only the spec, so accept a bare spec
                // name (e.g. `HTML`). Only a bare name (no `#`, not a URL) may
                // skip parsing; a malformed SPEC#anchor or unrecognized URL
                // still surfaces parse_spec_anchor's clear error rather than
                // failing later as an opaque "Unknown spec".
                let spec_name = webspec_index::diff_spec_name(&spec_anchor)?;
                let result = webspec_index::pr_diff(&spec_name, opts).await?;
                print_output(&cli.format, &result, format::pr_diff);
                return Ok(ExitCode::SUCCESS);
            }
            let options = webspec_index::effects::EffectsOptions {
                mode: effects_mode(effect_options.effects, view.is_some()),
                rule_paths: effect_options.rules.clone(),
                environment: effect_options.environment,
                ..Default::default()
            };
            let mut result = webspec_index::effects::query_section_with_effects(
                &spec_anchor,
                pr_opts.as_ref(),
                options,
            )
            .await?;
            let slice = match &view {
                Some(view) => {
                    let conn = rusqlite::Connection::open(webspec_index::db::get_db_path())?;
                    Some(apply_view(&conn, &mut result.query, view).map_err(|e| anyhow!("{e}"))?)
                }
                None => None,
            };
            let links_mode: content_filter::LinksMode = links.into();
            if links_mode != content_filter::LinksMode::Full || no_notes {
                let registry = webspec_index::spec_registry::SpecRegistry::new();
                if let Some(content) = result.query.content.as_deref() {
                    result.query.content = Some(content_filter::transform_content(
                        content, links_mode, no_notes, &registry,
                    ));
                }
            }
            let spec_id = format!("{}#{}", result.query.spec, result.query.anchor);
            let state_hint = match pr {
                Some(_) => None,
                None => {
                    let conn = rusqlite::Connection::open(webspec_index::db::get_db_path())?;
                    webspec_index::state::query::state_hint(
                        &conn,
                        &result.query.spec,
                        &result.query.anchor,
                        result.query.section_type == "algorithm",
                    )?
                }
            };
            match cli.format {
                OutputFormat::Json => {
                    let mut v = serde_json::to_value(&result).context("serialization failed")?;
                    if let (Some(hint), Some(object)) = (&state_hint, v.as_object_mut()) {
                        object.insert(
                            "state".into(),
                            serde_json::to_value(hint).context("serialization failed")?,
                        );
                    }
                    if let (Some(slice), Some(object)) = (&slice, v.as_object_mut()) {
                        object.insert(
                            "slice".into(),
                            serde_json::to_value(slice).context("serialization failed")?,
                        );
                    }
                    apply_refs_summary(
                        &mut v,
                        &spec_id,
                        pr,
                        "outgoing_refs",
                        "outgoing",
                        result.query.outgoing_refs.len(),
                    );
                    apply_refs_summary(
                        &mut v,
                        &spec_id,
                        pr,
                        "incoming_refs",
                        "incoming",
                        result.query.incoming_refs.len(),
                    );
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&v).context("serialization failed")?
                    );
                }
                OutputFormat::Markdown => {
                    let catalog = result.effects.as_ref().and_then(|_| {
                        webspec_index::effects::default_catalog(&effect_options.rules).ok()
                    });
                    let markdown = match &slice {
                        Some(slice) => {
                            format::query_view_with_effects(&result, slice, catalog.as_ref(), pr)
                        }
                        None => format::query_with_effects(&result, catalog.as_ref(), pr),
                    };
                    print!("{markdown}");
                    if let Some(hint) = &state_hint {
                        println!("\n{}", format::state_hint(hint));
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Search {
            query,
            spec,
            limit,
            pr,
            force_update,
        } => {
            let pr_opts = pr.map(|n| model::PrOpts {
                pr_number: n,
                force_update,
            });
            let result =
                webspec_index::search_sections(&query, spec.as_deref(), limit, pr_opts.as_ref())
                    .await?;
            print_output(&cli.format, &result, format::search);
            Ok(ExitCode::SUCCESS)
        }

        Command::Exists {
            spec_anchor,
            pr,
            force_update,
        } => {
            let pr_opts = pr.map(|n| model::PrOpts {
                pr_number: n,
                force_update,
            });
            let result = webspec_index::check_exists(&spec_anchor, pr_opts.as_ref()).await?;
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
            pr,
            force_update,
        } => {
            let pr_opts = pr.map(|n| model::PrOpts {
                pr_number: n,
                force_update,
            });
            let result =
                webspec_index::find_anchors(&pattern, spec.as_deref(), limit, pr_opts.as_ref())
                    .await?;
            print_output(&cli.format, &result, format::anchors);
            Ok(ExitCode::SUCCESS)
        }

        Command::List {
            spec,
            pr,
            force_update,
        } => {
            let pr_opts = pr.map(|n| model::PrOpts {
                pr_number: n,
                force_update,
            });
            let result = webspec_index::list_headings(&spec, pr_opts.as_ref()).await?;
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
            target,
            direction,
            limit,
            pr,
            force_update,
            kind,
        } => {
            let pr_opts = pr.map(|n| model::PrOpts {
                pr_number: n,
                force_update,
            });
            let kind = parse_ref_kind(kind.as_deref())?;
            let result =
                webspec_index::find_references(&target, &direction, limit, pr_opts.as_ref(), kind)
                    .await?;
            print_output(&cli.format, &result, format::refs);
            Ok(ExitCode::SUCCESS)
        }

        Command::Trace {
            from,
            to,
            max_depth,
            kind,
            limit,
            detail,
        } => {
            let kind = if kind.eq_ignore_ascii_case("any") {
                None
            } else {
                parse_ref_kind(Some(&kind))?
            };
            let detail = detail.parse::<model::TraceDetail>().map_err(|_| {
                anyhow::anyhow!("unknown detail '{detail}' (verbose, edges, compact)")
            })?;
            let mut result = webspec_index::find_traces(&from, &to, max_depth, kind, limit).await?;
            result.apply_detail(detail);
            print_output(&cli.format, &result, |r| format::trace(r, detail));
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

        Command::Flow {
            spec_anchor,
            flow_format,
        } => {
            match webspec_index::flow_section(&spec_anchor).await? {
                Some(result) => match flow_format {
                    FlowOutputFormat::Json => {
                        println!("{}", serde_json::to_string_pretty(&result)?);
                    }
                    FlowOutputFormat::Mermaid => {
                        print!("{}", format::flow_mermaid(&result));
                    }
                },
                None => {
                    eprintln!("no algorithm steps in {spec_anchor}");
                    return Ok(ExitCode::FAILURE);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Idl {
            query,
            spec,
            limit,
            pr,
            force_update,
        } => {
            let pr_opts = pr.map(|n| model::PrOpts {
                pr_number: n,
                force_update,
            });
            let result =
                webspec_index::query_idl(&query, spec.as_deref(), limit, pr_opts.as_ref()).await?;
            print_output(&cli.format, &result, format::idl);
            Ok(ExitCode::SUCCESS)
        }

        Command::State {
            selector,
            no_inits,
            unclassified,
            calls,
            callers,
            opaque,
            limit,
            rules,
            coverage,
        } => {
            if let Some(spec) = coverage {
                match webspec_index::state::service::coverage(&spec).await? {
                    Ok(result) => {
                        print_output(&cli.format, &result, webspec_index::state::render::coverage);
                        Ok(ExitCode::SUCCESS)
                    }
                    Err(error) => {
                        eprintln!("Error: {}", error.message);
                        Ok(ExitCode::FAILURE)
                    }
                }
            } else {
                let selector =
                    selector.expect("clap ensures selector is Some when coverage is None");
                let options = webspec_index::state::query::StateQueryOptions {
                    include_inits: !no_inits,
                    unclassified,
                    limit,
                    calls,
                    callers,
                    opaque,
                };
                match webspec_index::state::service::state(&selector, &options, &rules).await? {
                    Ok(result) => {
                        print_output(&cli.format, &result, webspec_index::state::render::response);
                        Ok(ExitCode::SUCCESS)
                    }
                    Err(error) => {
                        eprintln!("Error: {}", error.message);
                        for candidate in &error.candidates {
                            eprintln!("  {candidate}");
                        }
                        Ok(ExitCode::FAILURE)
                    }
                }
            }
        }

        Command::Update {
            spec,
            force,
            refetch,
            providers,
            effect_options,
        } => {
            let conn = webspec_index::db::open_or_create_db()?;
            let lock = webspec_index::refresh::hold_lock(&conn).await?;
            let results = webspec_index::refresh::while_holding(&lock, async {
                let results =
                    webspec_index::update_specs(spec.as_deref(), force, refetch, &providers)
                        .await?;
                if effect_options.effects == UpdateEffectsMode::Auto {
                    recompute_effects(effect_options.rules, effect_options.environment)?;
                }
                anyhow::Ok(results)
            })
            .await?;
            drop(lock);
            webspec_index::refresh_planner_stats();
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

        Command::Reparse {
            spec,
            providers,
            effect_options,
        } => {
            let conn = webspec_index::db::open_or_create_db()?;
            let lock = webspec_index::refresh::hold_lock(&conn).await?;
            let results = webspec_index::refresh::while_holding(&lock, async {
                let results = webspec_index::reparse_specs(spec.as_deref(), &providers).await?;
                if effect_options.effects == UpdateEffectsMode::Auto {
                    recompute_effects(effect_options.rules, effect_options.environment)?;
                }
                anyhow::Ok(results)
            })
            .await?;
            drop(lock);
            webspec_index::refresh_planner_stats();
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

        Command::Effects(args) => {
            let EffectsArgs {
                subject,
                all,
                rebuild,
                summary_only,
                compact,
                step,
                step_id,
                body_id,
                kind,
                category,
                rule_id,
                effect_id,
                occurrence_id,
                rules,
                environment,
                max_bodies,
                max_relationships,
                max_states,
                max_depth,
                max_witness_states,
                limit,
            } = *args;
            use webspec_index::effects;

            let has_filter = kind.is_some()
                || category.is_some()
                || rule_id.is_some()
                || effect_id.is_some()
                || occurrence_id.is_some();
            if all && (step.is_some() || step_id.is_some() || body_id.is_some() || has_filter) {
                anyhow::bail!("--all cannot be combined with subject selectors or filters");
            }
            // clap's `requires = "all"` accepts `--rebuild` alone, so check it here.
            if rebuild && !all {
                anyhow::bail!("--rebuild requires --all");
            }

            let budgets = effects::DiscoveryBudgets {
                max_bodies,
                max_relationships,
                max_states,
            };
            let options = effects::EffectsOptions {
                mode: effects::EffectsMode::Auto,
                rule_paths: rules.clone(),
                environment,
                budgets,
            };

            if all {
                let request = effects::RecomputeEffectsRequest {
                    schema_version: effects::EFFECTS_SCHEMA_VERSION,
                    options,
                };
                let conn = webspec_index::db::open_or_create_db()?;
                let lock = webspec_index::refresh::hold_lock(&conn).await?;
                let result = if rebuild {
                    effects::rebuild_effects(&request)?
                } else {
                    effects::recompute_effects(&request)?
                };
                drop(lock);
                print_output(&cli.format, &result, |_| {
                    format!(
                        "Effects graph built: {} bodies, {} relationships.\n",
                        result.body_count, result.relationship_count,
                    )
                });
                return Ok(ExitCode::SUCCESS);
            }

            let subject_text = subject.context("a subject or --all is required")?;
            let (parsed_spec, parsed_anchor, _) = webspec_index::parse_spec_anchor(&subject_text)?;
            let step_path = step.as_deref().map(parse_step_path).transpose()?;
            let mut selector = effects::SubjectSelector {
                spec: parsed_spec,
                anchor: parsed_anchor,
                step_path,
                step_id,
                body_id,
            };

            let queried = webspec_index::query_section(&subject_text, None).await?;
            selector.spec = queried.spec;
            selector.anchor = queried.anchor;

            let filter = has_filter.then_some(effects::EffectFilter {
                kind,
                category,
                rule_id,
                effect_id,
                occurrence_id,
            });
            let catalog = effects::default_catalog(&rules)?;
            if summary_only || compact {
                let request = effects::EffectsRequest {
                    schema_version: effects::EFFECTS_SCHEMA_VERSION,
                    subject: selector,
                    options,
                    filter,
                };
                let mut result = if compact {
                    effects::get_effect_preview(&request)?
                } else {
                    effects::get_effect_summary(&request)?
                };
                if compact {
                    result = effects::query::compact_summary(result);
                }
                print_output(&cli.format, &result, |result| {
                    effects::render::summary_markdown(result, Some(&catalog))
                });
            } else {
                let result = effects::explain_effects(&effects::ExplainEffectsRequest {
                    schema_version: effects::EFFECTS_SCHEMA_VERSION,
                    subject: selector,
                    options,
                    filter,
                    explanation: effects::ExplanationOptions {
                        max_depth,
                        max_states: max_witness_states,
                        limit,
                    },
                })?;
                print_output(&cli.format, &result, |result| {
                    effects::render::explanation_markdown(result, Some(&catalog))
                });
            }
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
            sections_output,
        } => {
            run_analyze(
                &path,
                recursive,
                threshold,
                &output_format,
                output_dir.as_deref(),
                strip_prefix.as_deref(),
                sections_output.as_deref(),
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Specs => {
            let urls = webspec_index::spec_urls();
            println!("{}", serde_json::to_string_pretty(&urls)?);
            Ok(ExitCode::SUCCESS)
        }

        Command::Lsp { rules, environment } => {
            webspec_index::lsp::serve_stdio_with_options(webspec_index::lsp::LspOptions {
                rule_paths: rules,
                environment,
                ..Default::default()
            })
            .await;
            Ok(ExitCode::SUCCESS)
        }

        Command::ClearPr { spec, pr, all } => {
            let result = webspec_index::clear_pr_data(spec.as_deref(), pr, all)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(ExitCode::SUCCESS)
        }

        Command::ExportWeb {
            out,
            providers,
            specs,
            chunk_size,
            max_size,
        } => {
            let options = webspec_index::export::ExportOptions {
                providers,
                specs,
                chunk_size,
                max_total_bytes: max_size,
                ..Default::default()
            };
            let manifest = webspec_index::export::export_web(
                &webspec_index::db::get_db_path(),
                &out,
                &options,
            )?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
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

/// Run the analyze subcommand.
async fn run_analyze(
    path: &std::path::Path,
    recursive: bool,
    threshold: f64,
    format: &AnalyzeFormat,
    output_dir: Option<&std::path::Path>,
    strip_prefix: Option<&std::path::Path>,
    sections_output: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    use webspec_index::analyze::file::FileAnalysisView;
    use webspec_index::analyze::searchfox::to_searchfox_records;

    if (output_dir.is_some() || sections_output.is_some())
        && !matches!(format, AnalyzeFormat::Searchfox)
    {
        anyhow::bail!("--output-dir/--sections-output require --output-format=searchfox");
    }

    let run =
        webspec_index::analyze::orchestrate::analyze_paths(path, recursive, threshold).await?;

    if run.total_files_scanned == 0 {
        eprintln!("No source files found in {}", path.display());
        return Ok(());
    }

    for (file_path, err) in &run.read_errors {
        eprintln!("warning: {}: {err}", file_path.display());
    }

    let mut files_with_refs = 0;

    for analyzed in &run.files {
        let file_path = &analyzed.path;
        let view = FileAnalysisView::from(&analyzed.analysis);

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

    let sections_path = sections_output
        .map(|p| p.to_path_buf())
        .or_else(|| output_dir.map(|d| d.join("spec-sections.json")));
    if let Some(sections_path) = sections_path {
        let sections = &run.resolved_sections;
        if !sections.is_empty() {
            if let Some(parent) = sections_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string(sections)?;
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

/// Transform a `refs` array field in a serialised `QueryWithEffects` value into
/// the summarised shape.
///
/// When `total < REFS_LIST_THRESHOLD` the field becomes `{"total": N, "items": [...]}`.
/// When `total >= REFS_LIST_THRESHOLD` the array is replaced with
/// `{"total": N, "command": "webspec-index refs ..."}` so the caller has a
/// ready-made command to fetch the full list.
fn apply_refs_summary(
    v: &mut serde_json::Value,
    spec_id: &str,
    pr: Option<i64>,
    field: &str,
    direction: &str,
    total: usize,
) {
    let items = v[field].take();
    if total < format::REFS_LIST_THRESHOLD {
        v[field] = serde_json::json!({"total": total, "items": items});
    } else {
        let cmd = format::refs_command(spec_id, direction, total, pr);
        v[field] = serde_json::json!({"total": total, "command": cmd});
    }
}

/// Publish effects incrementally after `update`/`reparse`.
fn recompute_effects(rule_paths: Vec<String>, environment: String) -> anyhow::Result<()> {
    webspec_index::effects::recompute_effects(&webspec_index::effects::RecomputeEffectsRequest {
        schema_version: webspec_index::effects::EFFECTS_SCHEMA_VERSION,
        options: webspec_index::effects::EffectsOptions {
            rule_paths,
            environment,
            ..Default::default()
        },
    })?;
    Ok(())
}

fn parse_ref_kind(kind: Option<&str>) -> anyhow::Result<Option<model::RefKind>> {
    match kind {
        None => Ok(None),
        Some(k) => k
            .parse::<model::RefKind>()
            .map(Some)
            .map_err(|_| anyhow::anyhow!("unknown reference kind '{k}' (step, note, idl, prose)")),
    }
}

fn parse_step_path(value: &str) -> anyhow::Result<Vec<u32>> {
    if value.is_empty() {
        anyhow::bail!("--step must be a dotted sequence of nonnegative integers");
    }
    value
        .split('.')
        .map(|part| {
            if part.is_empty() {
                anyhow::bail!("--step must be a dotted sequence of nonnegative integers");
            }
            part.parse::<u32>().map_err(|_| {
                anyhow::anyhow!("--step must be a dotted sequence of nonnegative integers")
            })
        })
        .collect()
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

#[cfg(test)]
mod cli_effect_tests {
    use super::*;

    #[test]
    fn compact_effects_is_a_distinct_summary_mode() {
        let cli = Cli::try_parse_from(["webspec-index", "effects", "HTML#navigate", "--compact"])
            .unwrap();
        let Command::Effects(args) = cli.command else {
            panic!("expected effects command")
        };
        assert!(args.compact);
        assert!(!args.summary_only);
        assert!(Cli::try_parse_from([
            "webspec-index",
            "effects",
            "HTML#navigate",
            "--compact",
            "--summary-only"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "webspec-index",
            "effects",
            "--all",
            "--summary-only",
            "--compact"
        ])
        .is_err());
    }

    #[test]
    fn state_algorithm_view_flags_are_accepted() {
        let cli = Cli::try_parse_from([
            "webspec-index",
            "state",
            "HTML#navigate",
            "--calls",
            "--callers",
            "--opaque",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::State {
                calls: true,
                callers: true,
                opaque: true,
                ..
            }
        ));
        let cli = Cli::try_parse_from(["webspec-index", "state", "HTML#navigate"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::State {
                calls: false,
                callers: false,
                opaque: false,
                ..
            }
        ));
    }

    #[test]
    fn query_effects_off_is_accepted() {
        let cli = Cli::try_parse_from([
            "webspec-index",
            "query",
            "HTML#navigate",
            "--effects",
            "off",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Query {
                effect_options: QueryEffectsArgs {
                    effects: Some(EffectsModeArg::Off),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn query_view_flags_parse_and_conflict() {
        let cli = Cli::try_parse_from([
            "webspec-index",
            "query",
            "HTML#navigate",
            "--involving",
            "url,historyHandling",
            "--depth",
            "1",
        ])
        .unwrap();
        let Command::Query {
            involving,
            depth,
            effect_options,
            ..
        } = cli.command
        else {
            panic!()
        };
        assert_eq!(involving, ["url", "historyHandling"]);
        assert_eq!(depth, Some(1));
        assert_eq!(effect_options.effects, None);
        for args in [
            vec!["--involving", "x", "--steps", "1"],
            vec!["--feeding", "3", "--involving", "x"],
            vec!["--steps", "1", "--diff"],
            vec!["--depth", "0"],
        ] {
            let mut all = vec!["webspec-index", "query", "HTML#navigate"];
            all.extend(args.iter().copied());
            assert!(Cli::try_parse_from(all.clone()).is_err(), "{all:?}");
        }
    }

    #[test]
    fn view_request_and_effects_default() {
        assert_eq!(view_request(vec![], None, vec![], None).unwrap(), None);
        let v = view_request(vec![], Some("24.9.1:historyEntry".into()), vec![], None)
            .unwrap()
            .unwrap();
        assert_eq!(v.feeding.unwrap().variables, ["historyEntry"]);
        assert!(view_request(vec![], Some("x.y".into()), vec![], None)
            .unwrap_err()
            .to_string()
            .starts_with("slice_invalid_selector"));
        for (involving, steps) in [
            (vec![], vec!["abc".to_string()]),
            (vec!["".to_string()], vec![]),
            (vec!["*".to_string()], vec![]),
        ] {
            let e = view_request(involving.clone(), None, steps.clone(), None)
                .unwrap_err()
                .to_string();
            assert!(
                e.starts_with("slice_invalid_selector"),
                "{involving:?} {steps:?}: {e}"
            );
        }
        let v = view_request(vec!["*url*".into(), "url".into()], None, vec![], Some(1))
            .unwrap()
            .unwrap();
        assert_eq!(v.involving, ["url"], "normalized before the lookup");
        use webspec_index::effects::EffectsMode;
        assert_eq!(effects_mode(None, false), EffectsMode::Auto);
        assert_eq!(effects_mode(None, true), EffectsMode::Off);
        assert_eq!(
            effects_mode(Some(EffectsModeArg::Cached), true),
            EffectsMode::Cached
        );
    }

    #[test]
    fn effects_rejects_conflicting_subject_selectors() {
        let result = Cli::try_parse_from([
            "webspec-index",
            "effects",
            "HTML#navigate",
            "--step",
            "2",
            "--step-id",
            "st_exact",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn all_accepts_bare_invocation() {
        let result = Cli::try_parse_from(["webspec-index", "effects", "--all"]);
        assert!(result.is_ok());
    }

    #[test]
    fn numeric_limits_must_be_positive() {
        let result = Cli::try_parse_from([
            "webspec-index",
            "effects",
            "HTML#navigate",
            "--max-depth",
            "0",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn parses_dotted_step_path() {
        assert_eq!(parse_step_path("14.12.3").unwrap(), vec![14, 12, 3]);
        assert!(parse_step_path("14..3").is_err());
    }

    #[test]
    fn query_links_default_is_short() {
        let cli = Cli::try_parse_from(["webspec-index", "query", "HTML#navigate"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Query {
                links: LinksArg::Short,
                no_notes: false,
                ..
            }
        ));
    }

    #[test]
    fn query_links_full_is_accepted() {
        let cli =
            Cli::try_parse_from(["webspec-index", "query", "HTML#navigate", "--links", "full"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Query {
                links: LinksArg::Full,
                ..
            }
        ));
    }

    #[test]
    fn query_links_none_is_accepted() {
        let cli =
            Cli::try_parse_from(["webspec-index", "query", "HTML#navigate", "--links", "none"])
                .unwrap();
        assert!(matches!(
            cli.command,
            Command::Query {
                links: LinksArg::None,
                ..
            }
        ));
    }

    #[test]
    fn query_no_notes_flag_is_accepted() {
        let cli =
            Cli::try_parse_from(["webspec-index", "query", "HTML#navigate", "--no-notes"]).unwrap();
        assert!(matches!(cli.command, Command::Query { no_notes: true, .. }));
    }
}
