# webspec-lens — Firefox Extension

Hover over spec URLs in Phabricator diffs and Searchfox source views to see the
referenced spec section inline.

## Installation

### 1. Install the binary

```bash
cargo binstall webspec-index   # or: cargo install webspec-index
```

### 2. Register the native messaging host

```bash
bash editors/firefox/install.sh
```

This writes `~/.mozilla/native-messaging-hosts/webspec_index.json` (Linux) or
`~/Library/Application Support/Mozilla/NativeMessagingHosts/webspec_index.json`
(macOS) pointing at the `webspec-index` binary.

### 3. Load the extension

In Firefox: go to `about:debugging` → **This Firefox** → **Load Temporary Add-on** →
select `editors/firefox/manifest.json`.

For permanent installation, the extension must be signed by Mozilla or loaded via
a Developer Edition / Nightly with `xpinstall.signatures.required` set to `false`.

## What it does

The extension scans source code shown on:

- `https://phabricator.services.mozilla.com/` — Differential diffs and file views
- `https://searchfox.org/` — File source view

Spec URLs found in code are turned into links. Hovering one fetches the spec
section from the local `webspec-index` database and shows it in a floating popup.

The first query for a spec triggers a background fetch and index (a few seconds).
Subsequent queries for the same spec are instant (local SQLite).

## Recognised URL format

Any URL matching a whitelisted spec domain with a `#fragment`:

| Domain pattern | Example |
|---|---|
| `*.spec.whatwg.org` | `https://html.spec.whatwg.org/#navigate` |
| `drafts.csswg.org` | `https://drafts.csswg.org/css-grid/#grid-container` |
| `drafts.fxtf.org` | `https://drafts.fxtf.org/filter-effects/#typedef-filter-function` |
| `www.w3.org/TR/<spec>` | `https://www.w3.org/TR/webaudio/#AudioContext` |
| `w3c.github.io/<spec>` | `https://w3c.github.io/webappsec-csp/#directive-default-src` |
| `wicg.github.io/<spec>` | `https://wicg.github.io/nav-speculation/#speculation-rules` |
| `webaudio.github.io/<spec>` | `https://webaudio.github.io/web-audio-api/#AudioNode` |
| `tc39.es/ecma<N>` | `https://tc39.es/ecma262/#sec-toprimitive` |
| `webassembly.github.io/<spec>` | `https://webassembly.github.io/spec/core/#valid` |
| `www.rfc-editor.org/rfc/<rfc>` | `https://www.rfc-editor.org/rfc/rfc9110.html#section-5` |
| `datatracker.ietf.org/doc/html/<rfc>` | `https://datatracker.ietf.org/doc/html/rfc9110#section-5` |

Multipage WHATWG URLs are also accepted:
`https://html.spec.whatwg.org/multipage/browsing-the-web.html#navigate`

## Hover popup content

The popup shows:

- **Section title** (e.g. "navigate")
- **Type and anchor** (e.g. `Algorithm · HTML#navigate`)
- **Section content** — the full text of the algorithm, definition, or heading,
  rendered from stored markdown

Markdown conventions used in spec content:

- `**text**` — bold (term being defined, important concept)
- `` `code` `` — inline code (IDL names, attribute values, algorithm variables)
- `## Heading` — section heading
- Numbered lists — algorithm steps

## Step comment format

The `webspec-index analyze` CLI (and the LSP server) also recognises *step
comments* in source code and validates them against the referenced spec
algorithm. Step comments are matched by:

```
// Step N.      — single step with trailing dot
// Step N.M.    — sub-step
// N.           — step number with trailing dot, no "Step" keyword
// N.M          — multi-part number (no prefix or dot required)
/* Step N. … */ — C block comment
# Step N. …    — Python / shell comment
; Step N. …    — assembly comment
```

Step comments must appear inside the indentation scope of a spec URL comment
to be associated with that algorithm.

## Architecture

```
Firefox extension (content.js)
  │  mouseenter on spec URL span
  ▼
background.js  ──sendMessage──►  Native Messaging port
                                        │  4-byte LE length + JSON
                                        ▼
                              webspec-index native-messaging
                                        │  query_section(url)
                                        ▼
                              local SQLite database
                               (fetches from network on first use)
```

### Native messaging protocol

Request (JSON, one per message):
```json
{"id": 1, "url": "https://html.spec.whatwg.org/#navigate"}
```

Response on success:
```json
{
  "id": 1,
  "ok": true,
  "spec": "HTML",
  "anchor": "navigate",
  "title": "navigate",
  "section_type": "Algorithm",
  "content": "To **navigate** a [navigable]…"
}
```

Response on error:
```json
{"id": 1, "ok": false, "error": "Section not found: HTML#nonexistent"}
```

Messages are framed with a 4-byte little-endian unsigned integer length prefix,
matching the [Firefox Native Messaging protocol](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_messaging).
