---
name: webspec-index
description: "Look up HTML specs, CSS specifications, ECMAScript proposals, and web standards using webspec-index. Query WHATWG, W3C, and TC39 spec sections, search for browser API definitions, verify spec anchors, trace cross-references, and retrieve WebIDL interfaces from the command line. Use when implementing web platform features, checking what a spec algorithm says, validating spec anchor URLs in code comments, finding the correct spec section for a browser API, or understanding cross-spec dependencies."
---

# webspec-index

Query WHATWG, W3C, and TC39 web specifications from the command line. Specs are fetched and cached locally on first use.

## Installation

```bash
cargo binstall webspec-index
# or
cargo install webspec-index
```

## Commands

Always quote the section identifier to avoid shell interpretation of `#`. See `webspec-index --help` for full options.

### Look up a spec section

```bash
webspec-index query 'HTML#navigate'
webspec-index query 'DOM#concept-tree'
webspec-index query 'CSS-GRID#grid-container'
webspec-index query 'https://html.spec.whatwg.org/#navigate'
```

Returns title, type, content as markdown, navigation tree, and cross-references.
Use `--format markdown` for human-readable output, or `--format json` (default) for structured data.

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

Exit code 0 = found, 1 = not found. Validate anchor names before referencing them in code.

### Find anchors by pattern

```bash
webspec-index anchors "*-tree" --spec DOM
webspec-index anchors "concept-*" --spec HTML
```

Glob matching (`*` wildcard). Useful when you know part of an anchor name but not the exact id.

### List all sections in a spec

```bash
webspec-index list DOM
```

### Cross-references

```bash
webspec-index refs 'HTML#navigate' --direction incoming
webspec-index refs 'HTML#navigate' --direction outgoing
webspec-index refs 'Window.navigation' --limit 5
```

Shows which sections reference this one (incoming), which this one references (outgoing), or both (default). Targets can be exact (`SPEC#anchor`) or shorthand (`Interface.member`).

### Update specs

```bash
webspec-index update
webspec-index update --spec HTML --force
```

Fetches latest spec versions. Uses 24h cache unless `--force` is given. Specs are auto-fetched on first query.

### Graph traversal

```bash
webspec-index graph 'HTML#navigate' --direction outgoing --max-depth 2
webspec-index graph 'HTML#navigate' --graph-format mermaid
webspec-index graph 'HTML#navigate' --same-spec-only
webspec-index graph 'HTML#navigate' --include '*concept-*' --exclude 're:^URL#'
```

Builds a cross-reference graph rooted at a section. Supports JSON, Markdown, Mermaid, and Graphviz DOT output.

### Query WebIDL definitions

```bash
webspec-index idl 'Window.navigation'
webspec-index idl 'Window.open()'
webspec-index idl 'navigation' --spec HTML --limit 5
```

Queries structured WebIDL definitions. Supports exact anchors and canonical names (`Interface.member`, `Interface.method()`).
Use this when the task is about API shape or IDL ownership, then `refs` for algorithm usage.

## Workflow: implementing a spec algorithm

```bash
# 1. Read the algorithm
webspec-index query 'HTML#navigate' --format markdown

# 2. Check what concepts it references
webspec-index refs 'HTML#navigate' --direction outgoing

# 3. Look up an unfamiliar referenced concept
webspec-index query 'INFRA#ordered-set'

# 4. Verify an anchor before adding it to a code comment
webspec-index exists 'HTML#navigate'

# 5. See what other specs depend on a concept you're changing
webspec-index refs 'DOM#concept-tree' --direction incoming
```
