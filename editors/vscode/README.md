# webspec-lens

Hover over WHATWG/W3C/TC39 spec URLs in your code to see section content inline. Validate step comments against the spec algorithm and track implementation coverage.

## Features

### Spec URL hover

Hover any spec URL to see the section's rendered content without leaving your editor. Works in any file type — C++, Rust, JavaScript, Python, HTML, etc.

```cpp
// https://html.spec.whatwg.org/#navigate
//                                 ^ hover here to see the full algorithm
```

### Step validation

Step comments (e.g. `// 5.1. Let x be ...`) are matched against the spec algorithm. Mismatches and unknown steps show as warnings. Matching steps get a checkmark inlay hint.

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

```text
navigate: 7/23 steps | 2 warnings
```

## Setup

**No manual installation required.** If `webspec-index` is not found on your PATH, the extension will offer to download the correct binary for your platform automatically.

To install manually instead: `cargo binstall webspec-index` or `cargo install webspec-index`.

Spec data is fetched and cached automatically on first query — no setup step needed.

## How it works

The extension launches a lightweight LSP server (`webspec-index lsp`) over stdio. All spec data is queried from a local SQLite database. Specs are fetched on first access and cached locally, so there are no network requests during normal editing after the initial fetch.

The server is auto-detected in this order:

1. `webspecLens.serverCommand` setting (if configured)
2. `webspec-index` on PATH
3. Previously downloaded binary (auto-updated when the extension updates)

## Settings

| Setting                      | Default     | Description                                                    |
| ---------------------------- | ----------- | -------------------------------------------------------------- |
| `webspecLens.enabled`        | `true`      | Enable or disable the extension                                |
| `webspecLens.serverCommand`  | auto-detect | Command to start the LSP server                                |
| `webspecLens.fuzzyThreshold` | `0.85`      | Jaro-Winkler similarity threshold for step matching (0.0–1.0)  |

## License

MIT
