# webspec-index

Query WHATWG, W3C, and TC39 web specifications from the command line.

## Features

- **Full-text search** across HTML, DOM, URL, CSS, ECMAScript, and 70+ other specifications
- **Cross-reference tracking** — see incoming/outgoing references between spec sections
- **Graph traversal** — build cross-reference graphs with JSON, Mermaid, or Graphviz DOT output
- **Control-flow extraction** — `flow` command produces a flowchart of algorithm steps (branches, loops, terminals) with Mermaid export
- **PR previews** — query spec sections as modified by an open WHATWG or TC39 proposal PR, with section-level diffs
- **Auto URL indexing for whitelisted domains** — query non-hardcoded specs by URL
- **Fast SQLite indexing** with FTS5 for instant queries
- **Algorithm and IDL extraction** with rendered markdown content
- **LSP server** for inline spec hovers and step validation in your editor
- **LLM-friendly** `--help` output — automatically detected when run inside Claude Code, Codex, Gemini CLI, or OpenCode

## Installation

```bash
cargo binstall webspec-index
```

Or build from source:

```bash
cargo install webspec-index
```

## Quick Start

```bash
# Look up a spec section (algorithm, definition, heading, IDL)
webspec-index query "HTML#navigate"
webspec-index query "https://html.spec.whatwg.org/#navigate"
webspec-index query "https://w3c.github.io/webappsec-permissions-policy/#permissions-policy-header"
webspec-index query "DOM#concept-tree" --format markdown

# Full-text search
webspec-index search "tree order" --spec DOM

# Check if an anchor exists (exit code 0 = found, 1 = not found)
webspec-index exists "HTML#navigate"

# Find anchors by glob pattern
webspec-index anchors "*-tree" --spec DOM

# List all headings in a spec
webspec-index list HTML

# Cross-references (exact or shorthand)
webspec-index refs "HTML#navigate" --direction incoming
webspec-index refs "Window.navigation" --limit 5

# Graph traversal
webspec-index graph "HTML#navigate" --max-depth 2 --graph-format mermaid
webspec-index graph "HTML#navigate" --graph-format dot
webspec-index graph "HTML#navigate" --same-spec-only
webspec-index graph "HTML#navigate" --include "*concept-*" --exclude "re:^URL#"

# Query dedicated WebIDL definitions
webspec-index idl "HTML#dom-window-navigation"
webspec-index idl "Window.navigation"
webspec-index idl "Window.open()" --spec HTML

# Query field state: who writes a field, and under what shape
webspec-index state HTML#is-initial-about:blank --format markdown
webspec-index state "Document.*sandbox*"
webspec-index state Document

# Algorithms: signature and call template, the calls it makes, and who calls it (bound to its parameters)
webspec-index state HTML#navigate --calls --format markdown
webspec-index state DOM#concept-event-fire --callers -l 5 --format markdown

# Views: cut an algorithm to a focused subset — every omitted step is counted in a marker
webspec-index query HTML#navigate --depth 1 --format markdown          # outline: top-level steps only
webspec-index query HTML#navigate --involving historyHandling --format markdown  # steps that involve historyHandling and variables derived from it
webspec-index query HTML#navigate --feeding 24.9.1 --format markdown   # earlier steps that define the variables step 24.9.1 uses

# Query text with compact possible-effect summaries (default: up to 12 groups)
webspec-index query "HTML#navigate"
webspec-index query "HTML#navigate" --effects cached
webspec-index query "HTML#navigate" --effects off  # original JSON shape

# Link rendering in content: SPEC#anchor by default, which `query` accepts directly
webspec-index query "HTML#navigate" --links short    # SPEC#anchor links (default)
webspec-index query "HTML#navigate" --links full     # full absolute URLs
webspec-index query "HTML#navigate" --links none     # link text only, no URLs
webspec-index query "HTML#navigate" --no-notes       # drop Note/Example/Warning blocks

# Cross-reference counts in query output: listed when < 5, command otherwise
# JSON: {"total": 3, "items": [...]} or {"total": 117, "command": "webspec-index refs ..."}
# Fetch the full list with the emitted command, e.g.:
webspec-index refs "HTML#navigate" -d incoming -l 117

# Inspect effect evidence for an algorithm or one of its steps
webspec-index effects "HTML#navigate"
webspec-index effects "HTML#navigate" --step 20 --kind event.fire
webspec-index effects "HTML#navigate" --effect-id ef_0123456789abcdef --limit 5
webspec-index effects "HTML#navigate" --summary-only

# Rebuild effects for the indexed corpus. `update` and `reparse` do this
# automatically; per-subject results are computed from the stored graph at query time.
# `effects --all` is incremental — only specs whose structure changed are reprocessed.
# `effects --all --rebuild` builds the effects from scratch.
webspec-index effects --all
webspec-index effects --all --rebuild

# Export a chunked read-only database for the web UI (state tables included)
webspec-index export-web --out DIR                      # all providers
webspec-index export-web --out DIR --specs html,dom     # only HTML and DOM

# Update specs to latest versions
webspec-index update
webspec-index update --force            # re-parse from on-disk HTML cache (no network)
webspec-index update --force --refetch  # re-download everything
webspec-index update --providers whatwg,w3c,tc39   # limit to specific providers
webspec-index update --effects off  # skip the post-update effects refresh

# Re-parse from the on-disk HTML cache (no network)
webspec-index reparse
webspec-index reparse --spec HTML
webspec-index reparse --providers whatwg,w3c
```

Effect results describe behavior the selected specification subject may cause. A
partial result and its issue codes identify incomplete inputs or analysis. Effect
analysis does not fetch referenced specifications recursively; use `update` to
populate dependencies. Add local catalog packages with repeated `--rules PATH`
and select host mappings with `--environment NAME`. See the [effects guide](docs/effects.md)
for rule examples, API entry points, supported semantics and measured performance.

PR previews keep returning their requested section text, but effects are reported
as unavailable with `unsupported_preview` until exact preview analysis is supported.

### PR Previews

Query spec sections as they would look after a PR is merged. Two providers are supported:

- **WHATWG specs** — previews are lazily fetched from [whatpr.org](https://whatpr.org).
- **TC39 proposals** — resolved via the GitHub API; the PR's committed `index.html` build is fetched from the head repo, and the merge base from the base repo. (Reflects the *committed* `index.html`, so a PR that edits `spec.emu` without rebuilding will preview the stale build.)

Both are cached locally.

```bash
# Query a section from a PR preview (falls back to merge base for unchanged sections)
webspec-index query "HTML#navigate" --pr 12345
webspec-index query "proposal-defer-import-eval#sec-IsModuleSCCEvaluated" --pr 85 --format markdown

# Diff: see what sections the PR adds or modifies vs the merge base.
# With --diff the #anchor is optional — a bare spec name previews the whole PR.
webspec-index query "HTML#navigate" --pr 12345 --diff --format markdown
webspec-index query proposal-defer-import-eval --pr 85 --diff --format markdown

# Force re-fetch (e.g. after the PR is updated)
webspec-index query "HTML#navigate" --pr 12345 --force-update

# Other commands also support --pr
webspec-index exists "HTML#navigate" --pr 12345
webspec-index list HTML --pr 12345
webspec-index refs "HTML#navigate" --pr 12345
webspec-index search "OpaqueRange" --spec HTML --pr 12345
webspec-index anchors "*opaquerange*" --spec HTML --pr 12345

# Manage cached PR data (each PR caches rendered pages + merge base)
webspec-index clear-pr                          # list cached PRs
webspec-index clear-pr --spec HTML --pr 12345   # remove one
webspec-index clear-pr --all                    # remove all
```

All commands support `--format json` (default) or `--format markdown`.

Spec data is fetched and cached locally on first query — no setup needed. On each
subsequent query, the spec and its transitive dependencies are checked at most once
per 24 h; the first query of the day may take a few seconds. Re-indexing happens only
when the HTML content changed.

## Web UI

Browse WHATWG, W3C, and TC39 specifications at <https://jnjaeschke.github.io/webspec-index/>.

The site offers:
- **Full-text and section search** across all indexed specs
- **Cross-references** between specifications
- **Algorithm effects** with possible behavior summaries and witness examples
- **Trace recorder** — record your path through algorithms and export as markdown with shareable links
- **Diagram views** — interactive (pan, zoom, click-through) reference graphs, algorithm flowcharts built from the `flow` extraction, effect witness paths, and trace diagrams; every diagram exports as Mermaid text
- **Daily updates** from the latest spec snapshots via GitHub Actions
- **Zero server** — entirely static; the database is a chunked SQLite file served over HTTP Range requests and read through WebAssembly in your browser

### Navigation by URL

Any spec URL can be opened directly by prepending it to the site:

```
https://jnjaeschke.github.io/webspec-index/#/https://html.spec.whatwg.org/#navigate
```

Or use the short form:

```
https://jnjaeschke.github.io/webspec-index/#/HTML/navigate
```

### Running locally

Set up the site locally in three steps (after building the native binary):

```bash
./crates/webspec-index-wasm/build.sh
webspec-index effects --all --rebuild
webspec-index export-web --out web/public/db --providers whatwg,w3c,tc39
cd web && npm ci && npm run dev
```

## AI Agent Integration

### Skill files

Drop [skills/webspec-index/](skills/webspec-index/SKILL.md) into your repo to teach the agent how to use the CLI.

[skills/spec-trace/](skills/spec-trace/SKILL.md) builds on it: given a question about what a spec
specifies, it traces the algorithm call chain with `paths` and produces a reviewable verdict.

## Editor Integration

The **webspec-lens** extension provides inline spec hovers, step validation, and coverage tracking. Available for VS Code and any LSP-compatible editor.

See [editors/vscode/](editors/vscode/) and [editors/zed/](editors/zed/) for details.

## How It Works

1. **Fetches** spec HTML from WHATWG/W3C/TC39 spec URLs
2. **Parses** sections, algorithms, IDL definitions, and cross-references
3. **Indexes** in SQLite with FTS5 for fast full-text search
4. **Refreshes snapshots** on a 24h cadence with content-hash change detection
5. **PR previews** fetch the PR build and its merge base (WHATWG: rendered pages from whatpr.org + commit-snapshots; TC39 proposals: committed `index.html` from the head/base repos via the GitHub API), storing both as separate snapshots for querying and diffing
6. **State layer** — at index time, builds an object model (types, fields, owners, member sets) from structural declarations and a statement IR from algorithm step text, then joins them across specs at query time to list who writes each field
7. **Views** — views cut the stored algorithm markdown at step paths using a per-algorithm slice index built at index time from the step structure and the statement IR; every omitted step is counted in a marker

## Development

```bash
cargo test
cargo clippy        # lint
cargo fmt --check   # format check
```

The `native` feature (default) holds every OS and network dependency; `pdf` holds ITU PDF parsing. Both are off for the WebAssembly build. `webspec_index::api::handle_json` is the request/response entry point shared by the wasm build and editor clients; see `src/api.rs` for the request variants.

```text
cargo test --no-default-features --lib                          # OS/network-free build (used by the wasm target)
cargo check --no-default-features --lib --target wasm32-unknown-unknown
```

### Invariant fuzzer

`examples/spec_fuzz` re-reads every indexed section's source HTML from the local HTML cache and checks that the index conserves it: links, words, algorithm steps, references, IR links, link rewriting, `--no-notes`, query round trips. It opens `~/.webspec-index/index.db` read-only and never rebuilds it. A spec whose stored sections no longer match a fresh parse is skipped with a `webspec-index reparse --spec SPEC` hint; an index built by another version is refused.

```bash
cargo run --release --example spec_fuzz -- run                 # one pass over every testable spec; --spec, --invariant, --loop
cargo run --release --example spec_fuzz -- report artifacts/spec-fuzz/<run-id>
cargo run --release --example spec_fuzz -- replay artifacts/spec-fuzz/<run-id> C1-3f2a9c
cargo run --release --example spec_fuzz -- promote artifacts/spec-fuzz/<run-id> C1-3f2a9c --name my-bug --bug "what is wrong"
cargo test --release --example spec_fuzz                      # harness self-tests
```

A run writes `manifest.json`, `findings.jsonl` (one record per distinct defect) and `summary.json` to `artifacts/spec-fuzz/<run-id>/`. `promote` reduces a finding to a small fixture in `tests/fixtures/fuzz/`, which `tests/fuzz_regressions.rs` checks as a ratchet: `known_bug` fixtures must still fail, `fixed` ones must pass. Fixing a bug therefore means flipping its fixture to `fixed`. `promote --section SPEC#anchor --invariant C1 --status fixed` records a section that has no finding.

## License

MIT

## Links

- [GitHub Repository](https://github.com/jnjaeschke/webspec-index)
- [Issue Tracker](https://github.com/jnjaeschke/webspec-index/issues)
- [VS Code Extension](https://marketplace.visualstudio.com/items?itemName=jnjaeschke.webspec-lens)
