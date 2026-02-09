# webspec-index

Query WHATWG/W3C web specifications from the command line, Python code, or AI agents (MCP).

## Features

- **Full-text search** across HTML, DOM, URL, and other specifications
- **Cross-reference tracking** (incoming/outgoing references between specs)
- **Fast SQLite-based indexing** with FTS5 for instant queries
- **Three interfaces**: CLI, Python library, and MCP server for AI agents

## Installation

```bash
pip install webspec-index
```

Or run directly with `uvx` (no installation needed):

```bash
uvx webspec-index query HTML#navigate
```

If you install via `pip`, the `webspec-index` command is available globally.
With `uvx`, prefix every command with `uvx webspec-index` instead.

The examples below assume `pip install`.

## Quick Start

### Command Line

```bash
# Query a specific section
webspec-index query HTML#navigate

# Search across all specs
webspec-index search "tree order" --spec DOM

# Check if a section exists (exit code 0 = found, 1 = not found)
webspec-index exists HTML#navigate

# Find anchors by pattern
webspec-index anchors "*-tree" --spec DOM

# List all headings
webspec-index list HTML

# Get cross-references
webspec-index refs HTML#navigate --direction incoming

# Update to latest spec versions
webspec-index update --spec HTML

# Clear local database
webspec-index clear-db
```

Most commands support `--format json` (default) or `--format markdown`.

### Python Library

```python
import webspec_index

# Query a section
result = webspec_index.query("HTML#navigate")
print(result["title"])  # "navigate"
print(result["section_type"])  # "Algorithm"

# Search
results = webspec_index.search("tree order", spec="DOM", limit=5)
for r in results["results"]:
    print(f"{r['spec']}#{r['anchor']}: {r['snippet']}")

# Check existence
if webspec_index.exists("HTML#navigate"):
    print("Section found!")
```

### MCP Server (AI Agents)

Start the MCP server for use with Claude Code or other AI agents:

```bash
claude mcp add webspec-index -- uvx webspec-index mcp
```

## Available Specifications

Currently indexed:
- **HTML** - WHATWG HTML Living Standard
- **DOM** - WHATWG DOM Living Standard
- **URL** - WHATWG URL Living Standard
- **INFRA** - WHATWG Infra Living Standard

More specs (Fetch, Encoding, Streams, etc.) coming soon!

## How It Works

1. **Fetches** spec HTML from WHATWG/W3C GitHub repositories
2. **Parses** sections, algorithms, IDL definitions, and cross-references
3. **Indexes** in SQLite with FTS5 for fast full-text search
4. **Tracks versions** using git commit SHAs for reproducibility

## Development

Built with:
- **Rust** for fast parsing and indexing (scraper, rusqlite, reqwest)
- **PyO3** for zero-cost Python bindings
- **Maturin** for packaging
- **Click** for CLI
- **MCP** (Model Context Protocol) for AI agent integration

## License

MIT

## Links

- [GitHub Repository](https://github.com/jnjaeschke/webspec-index)
- [Issue Tracker](https://github.com/jnjaeschke/webspec-index/issues)
