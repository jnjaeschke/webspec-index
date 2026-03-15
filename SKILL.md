---
name: webspec-index
description: Use webspec-index to query WHATWG, W3C, and TC39 web specifications from the command line
---

# webspec-index

Query WHATWG, W3C, and TC39 web specifications from the command line.

Use `webspec-index` whenever you need to understand what a web spec says — algorithm steps, section content, cross-references, or whether a spec anchor exists. Specs are fetched and cached locally on first use.

## Available specs

Assume that all specs from WHATWG, W3C and TC39 are indexed. If in doubt, run `webspec-index specs` to list all spec names and their base URLs.

## Installation

If `webspec-index` is not already available in your environment, you can install it via cargo:

```bash
cargo binstall webspec-index
# or
cargo install webspec-index
```

## Commands

Always put the section identifier in quotes to avoid shell interpretation of `#`.

See `webspec-index --help` for full command list and options.

### Look up a spec section

```bash
webspec-index query 'HTML#navigate'
webspec-index query 'DOM#concept-tree'
webspec-index query 'CSS-GRID#grid-container'
webspec-index query 'https://html.spec.whatwg.org/#navigate'
webspec-index query 'https://w3c.github.io/webappsec-permissions-policy/#permissions-policy-header'
```

Returns the section's title, type (heading/algorithm/definition), full content as markdown, navigation tree (parent/prev/next/children), and cross-references. This is the primary command — use it to read what a spec section says.

Use `--format markdown` for human-readable output, or default `--format json` for structured data.
For non-hardcoded specs, URL queries are accepted for whitelisted domains (`*.spec.whatwg.org`, `drafts.csswg.org`, `w3c.github.io`, `wicg.github.io`, `webaudio.github.io`, `tc39.es`, and `w3.org/TR/*`).

### Search across specs

```bash
webspec-index search "tree order"
webspec-index search "navigate" --spec HTML --limit 5
```

Full-text search with snippets. Use `--spec` to narrow to one spec.

### Check if a section exists

```bash
webspec-index exists 'HTML#navigate'
```

Exit code 0 = found, 1 = not found. Use this to validate anchor names before referencing them.

### Find anchors by pattern

```bash
webspec-index anchors "*-tree" --spec DOM
webspec-index anchors "concept-*" --spec HTML
webspec-index anchors "dom-*assign*"
```

Glob matching (`*` wildcard). Useful when you know part of an anchor name but not the exact id.

### List all sections in a spec

```bash
webspec-index list DOM
```

Returns all heading-level sections with their anchors, titles, types, and depths.

### Cross-references

```bash
webspec-index refs 'HTML#navigate' --direction incoming
webspec-index refs 'HTML#navigate' --direction outgoing
webspec-index refs 'HTML#navigate'
webspec-index refs 'Window.navigation' --limit 5
```

Shows which sections reference this one (incoming), which sections this one references (outgoing), or both (default). Target can be exact (`SPEC#anchor` or full URL) or shorthand (`Interface.member`) resolved heuristically against currently indexed sections. Use `--limit` to cap results when using shorthand queries.

### Update specs

```bash
webspec-index update
webspec-index update --spec HTML
webspec-index update --force
```

Fetches latest spec versions. Uses 24h cache unless `--force` is given. Specs are auto-fetched on first query, so you rarely need this.
Specs are checked on a 24h cadence; re-indexing happens only when fetched HTML content changed.

### Graph traversal

```bash
webspec-index graph 'HTML#navigate' --direction outgoing --max-depth 2
webspec-index graph 'HTML#navigate' --graph-format mermaid
webspec-index graph 'HTML#navigate' --graph-format dot
webspec-index graph 'HTML#navigate' --same-spec-only
webspec-index graph 'HTML#navigate' --include '*concept-*' --exclude 're:^URL#'
```

Builds a cross-reference graph rooted at a section. Supports JSON (default), Markdown, Mermaid, and Graphviz DOT output.
Use `--include` and `--exclude` to filter node ids (`SPEC#anchor`) by wildcard patterns (`*`, `?`) or regex (`re:<pattern>`).

### Query dedicated WebIDL definitions

```bash
webspec-index idl 'HTML#dom-window-navigation'
webspec-index idl 'Window.navigation'
webspec-index idl 'Window.open()'
webspec-index idl 'navigation' --spec HTML --limit 5
```

Queries structured WebIDL definitions directly. Supports exact anchors (`SPEC#anchor` or URL) and canonical names (`Interface.member`, `Interface.method()`).
Use this first when the task is about API shape or IDL ownership, then use `refs` to see algorithm usage.

## Usage patterns for Gecko development

### Understanding what you're implementing

When working on a bug that references a spec algorithm:

```bash
# Read the algorithm you need to implement
webspec-index query 'HTML#navigate' --format markdown

# Check what concepts it references
webspec-index refs 'HTML#navigate' --direction outgoing

# Look up a referenced concept you don't understand
webspec-index query 'INFRA#ordered-set'
```

### Finding the right spec section

When you see a spec URL in code comments (e.g., `https://html.spec.whatwg.org/#navigate`), or a step comment like `// Step 3.2`, query the section to understand the algorithm:

```bash
webspec-index query 'https://html.spec.whatwg.org/#navigate'
```

When you know a concept but not its exact anchor:

```bash
# Search by text
webspec-index search "tree order" --spec DOM

# Or find by anchor pattern
webspec-index anchors "*tree*order*" --spec DOM
```

### Verifying spec anchors

Before adding a spec URL to a code comment, verify the anchor exists:

```bash
webspec-index exists 'HTML#navigate' && echo "valid"
```

### Understanding cross-spec dependencies

To see what other specs depend on a concept you're changing:

```bash
webspec-index refs 'DOM#concept-tree' --direction incoming
```

### Tracing IDL API usage in algorithms

When implementing or reviewing a DOM API in Gecko:

```bash
# Find canonical IDL definition + owning interface
webspec-index idl 'Window.navigation' --format markdown

# Find where the property is used in indexed specs
webspec-index refs 'Window.navigation' --direction incoming
```
