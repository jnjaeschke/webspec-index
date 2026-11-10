# webspec-index-wasm

WebAssembly build of [webspec-index](../../README.md) that reads the exported
chunked database over HTTP range requests from a browser worker.

## Build

```sh
./build.sh
```

Requires `wasm-bindgen` 0.2.130 CLI and `clang` on `$PATH`. Output lands in
`pkg/`: `webspec_index_wasm.js` (ESM bindings) and `webspec_index_wasm_bg.wasm`.

## Exports

Three functions are exported to JavaScript:

### `open(manifest_url: string): void`

Fetches `manifest_url` synchronously (must be called from a worker), registers
the chunked database as a virtual file, and opens a read-only SQLite connection.

`manifest.json` must contain at least `{ "size": <bytes>, "chunk_size": <bytes> }`.
Chunk files (`0000.bin`, `0001.bin`, …) are resolved relative to the manifest URL.

Throws a `string` on any error (bad fetch, parse error, VFS registration failure,
SQLite open failure).

### `handle(request_json: string): string`

Runs a webspec-index API query and returns the JSON response.

If `open` has not been called, returns:

```json
{"type":"error","code":"not_open","message":"call open(manifest_url) first"}
```

### `stats(): string`

Returns a JSON object with cache and fetch statistics for the open database:

```json
{"fetches": 3, "bytes_fetched": 196608, "cache_hits": 14}
```

Returns `null` if no database is open.

## Worker requirement

`open` and the underlying range requests use synchronous XHR, which is only
available in dedicated workers (`new Worker(url, { type: "module" })`). Calling
`open` from the main thread throws.

## Generate a fixture export

```sh
cargo run --example fixture_export -- target/fixture-export
```

Writes `manifest.json` and `*.bin` chunk files to `target/fixture-export/`.
Useful for local testing of the wasm module without a full production database.

## End-to-end test

`e2e/` contains a headless-Firefox end-to-end test that exercises the full
wasm package against the fixture export.

From the repository root (the fixture example belongs to the root crate):

```sh
./crates/webspec-index-wasm/build.sh
cargo run --example fixture_export -- target/fixture-export
cd crates/webspec-index-wasm/e2e
npm ci
npx playwright install firefox
node run.mjs ../../../target/fixture-export
```

`run.mjs` accepts an optional second argument to switch browsers:

```sh
node run.mjs ../../../target/fixture-export chromium
```

On success it prints `e2e ok {"fetches":N,...}` and exits 0.
