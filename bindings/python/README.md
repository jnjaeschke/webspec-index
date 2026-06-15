# webspec-index (Python bindings)

Python bindings for [`webspec-index`](https://github.com/jnjaeschke/webspec-index) —
query WHATWG, W3C, TC39, and IETF web specifications: sections, cross-references,
WebIDL definitions, reference graphs, full-text search, WHATWG PR previews, and
source-file step-comment analysis.

The bindings wrap the same Rust core as the `webspec-index` CLI. All functions are
**synchronous**; spec data is fetched and cached locally on first use.

## Install

```bash
pip install webspec-index
```

## Usage

```python
import webspec_index as wsi

# Query a section (SPEC#anchor or a full URL)
section = wsi.query("HTML#navigate")
print(section.title, section.section_type)
for ref in section.outgoing_refs:
    print(ref.spec, ref.anchor)

# Full-text search within a spec
results = wsi.search("tree order", spec="DOM", limit=5)
for hit in results.results:
    print(hit.anchor, hit.snippet)

# Existence check
print(wsi.exists("HTML#navigate").exists)

# Anchors by glob, headings, cross-references, WebIDL, graph
wsi.anchors("*-tree", spec="DOM")
wsi.list_headings("DOM")
wsi.refs("Window.navigation", direction="incoming")
wsi.idl("Window.open()")
wsi.graph("HTML#navigate", max_depth=2)

# WHATWG PR previews
wsi.query("HTML#navigate", pr=12345)
wsi.pr_diff("HTML", pr=12345)

# Source analysis
for file in wsi.analyze("src/", recursive=True):
    print(file.file, len(file.scopes))

# Every result object is typed and also offers .to_dict() / .to_json()
section.to_dict()
```

Errors are raised as `webspec_index.WebspecError`.

## Development

This project uses [`uv`](https://docs.astral.sh/uv/) and
[`maturin`](https://www.maturin.rs/).

```bash
# from bindings/python/
uv sync --extra test     # build the extension + install test deps
uv run pytest            # run the test suite
```
