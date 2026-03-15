# webspec-lens for Zed

`webspec-lens` for Zed runs the same backend as the VS Code extension: `webspec-index lsp`.

## Features

- Spec URL hover (WHATWG/W3C/TC39)
- Step validation diagnostics
- Step validation inlay hints
- Coverage code lens summaries

## Backend resolution

The extension starts the language server in this order:

1. `webspec-index` on `PATH`
2. Managed binary downloaded from the latest `jnjaeschke/webspec-index` release for your platform

Managed binaries are installed under Zed's extension work directory.

## Notes

- The same `webspec-index` SQLite database is used (`~/.webspec-index/index.db`), so VS Code and Zed share cached spec data.
- The backend default fuzzy threshold is `0.85`, matching VS Code defaults.
