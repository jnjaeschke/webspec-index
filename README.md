# webspec-index

Query WHATWG, W3C, and TC39 web specifications from the command line.

## Features

- **Full-text search** across HTML, DOM, URL, CSS, ECMAScript, and 70+ other specifications
- **Cross-reference tracking** — see incoming/outgoing references between spec sections
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
webspec-index query "DOM#concept-tree" --format markdown

# Full-text search
webspec-index search "tree order" --spec DOM

# Check if an anchor exists (exit code 0 = found, 1 = not found)
webspec-index exists "HTML#navigate"

# Find anchors by glob pattern
webspec-index anchors "*-tree" --spec DOM

# List all headings in a spec
webspec-index list HTML

# Cross-references
webspec-index refs "HTML#navigate" --direction incoming

# Update specs to latest versions
webspec-index update
```

All commands support `--format json` (default) or `--format markdown`.

Spec data is fetched and cached locally on first query — no setup needed.

## AI Agent Integration

### Skill file

Drop [SKILL.md](SKILL.md) into your repo to teach the agent how to use the CLI.

## Editor Integration

The **webspec-lens** extension provides inline spec hovers, step validation, and coverage tracking. Available for VS Code and any LSP-compatible editor.

See [editors/vscode/](editors/vscode/) for details.

## How It Works

1. **Fetches** spec HTML from WHATWG/W3C/TC39 GitHub repositories
2. **Parses** sections, algorithms, IDL definitions, and cross-references
3. **Indexes** in SQLite with FTS5 for fast full-text search
4. **Tracks versions** using git commit SHAs for reproducibility

## Development

```bash
cargo test          # 235 tests
cargo clippy        # lint
cargo fmt --check   # format check
```

## License

MIT

## Links

- [GitHub Repository](https://github.com/jnjaeschke/webspec-index)
- [Issue Tracker](https://github.com/jnjaeschke/webspec-index/issues)
- [VS Code Extension](https://marketplace.visualstudio.com/items?itemName=jnjaeschke.webspec-lens)
