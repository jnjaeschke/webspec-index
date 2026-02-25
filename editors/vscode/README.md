# spec-lens

Hover over WHATWG spec URLs in your code to see section content inline. Validate step comments against the spec algorithm and track implementation coverage.

## Features

### Spec URL hover

Hover any WHATWG spec URL to see the section's rendered content without leaving your editor. Works in any file type — C++, Rust, JavaScript, Python, HTML, etc.

```cpp
// https://html.spec.whatwg.org/#navigate
//                                 ^ hover here to see the full algorithm
```

### Step validation

Step comments (e.g. `// Step 5.1. Let x be ...`) are matched against the spec algorithm. Mismatches and unknown steps show as warnings. Matching steps get a checkmark inlay hint.

```cpp
// https://html.spec.whatwg.org/#navigate
void DoNavigate(...) {
  // Step 1. Let cspNavigationType be ...    ✓  (matches spec)
  // Step 5.1. Assert: userInvolvement is    ⚠  (text differs)
  // Step 99. Nonexistent step               ⚠  (not in spec)
}
```

### Coverage code lens

A code lens above each spec URL shows how many algorithm steps are implemented:

```
navigate: 7/23 steps | 2 warnings
```

## Setup

**No manual installation required** if you have [uv](https://docs.astral.sh/uv/) installed. The extension auto-discovers the LSP server using `uvx`, which runs `webspec-index` directly from PyPI without a persistent install.

If you don't have `uv`, install the server manually:

```sh
pip install 'webspec-index[lsp]'
```

Spec data is fetched and cached automatically on first query — no setup step needed.

## How it works

The extension launches a lightweight LSP server (`webspec-index lsp`) over stdio. All spec data is queried from a local SQLite database. Specs are fetched on first access and cached locally, so there are no network requests during normal editing after the initial fetch.

The server is auto-detected in this order:

1. `specLens.serverCommand` setting (if configured)
2. `webspec-index` on PATH
3. `uvx webspec-index[lsp] lsp` (zero-install via uv)
4. `python -m webspec_index lsp`

## Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `specLens.enabled` | `true` | Enable or disable the extension |
| `specLens.serverCommand` | auto-detect | Command to start the LSP server |
| `specLens.fuzzyThreshold` | `0.85` | Jaro-Winkler similarity threshold for step matching (0.0-1.0) |

## Supported specs

All [WHATWG living standards](https://spec.whatwg.org/) — HTML, DOM, URL, Fetch, Streams, Encoding, and more.

## Other editors

The LSP server works with any editor that supports the Language Server Protocol:

**Neovim:**
```lua
vim.lsp.start({ cmd = { "webspec-index", "lsp" } })
```

**Zed:** add to `settings.json`:
```json
{
  "lsp": {
    "spec-lens": {
      "binary": { "path": "webspec-index", "arguments": ["lsp"] }
    }
  }
}
```

## License

MIT
