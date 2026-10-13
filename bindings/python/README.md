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

Field state (who writes a spec field):

```python
result = wsi.state("HTML#is-initial-about:blank")
print(result["field"]["name"], result["status"]["coverage"])
for site in result["writes"]:
    print(site["subject"], site["step_path"], site["text"])

coverage = wsi.state_coverage("HTML")
print(coverage["counters"]["owner_resolved"], "/", coverage["counters"]["written_fields"])
```

`state(selector, include_inits=True, unclassified=False, limit=None, calls=False,
callers=False, opaque=False)` accepts the same
selectors as the CLI: `SPEC#anchor`, `TYPE`, `"TYPE.FIELD"`, `"TYPE.GLOB"`, or `"SPEC#GLOB"`.
`state_coverage(spec)` returns the stored coverage counters for one spec.

For an algorithm, request its signature and the calls it makes with `calls=True`:

```python
result = wsi.state("HTML#location-object-navigate", calls=True)
```

Possible specification effects use dedicated, versioned request dictionaries;
the existing `query()` result remains unchanged:

```python
request: wsi.EffectsRequest = {
    "schema_version": 1,
    "subject": {"spec": "WEBIDL", "anchor": "wait-for-all"},
}
summary = wsi.get_effect_summary(request)
for effect in summary["effects"]:
    print(effect["kind"], effect["execution"])

details = wsi.explain_effects({**request, "explanation": {"limit": 1}})
for explanation in details["explanations"]:
    print(explanation["effect_id"], explanation["witnesses"])
```

`get_effect_summary()` and `explain_effects()` are synchronous. Results are
JSON-compatible dictionaries following effects schema version 1. Use
`options.rule_paths` for additional local rule packages and `options.environment`
to select host implementation mappings.

`recompute_effects()` triggers an incremental effects rebuild for the indexed
corpus (equivalent to `webspec-index effects --all`). It returns when the
publication is up to date. When a spec's HTML was updated and effects have not
been rebuilt yet, `get_effect_summary()` raises `WebspecError` with a message
starting `snapshot_changed:`; call `recompute_effects()` once to rebuild, then
retry the query. From the CLI, `query` output carries
`"effects_status": {"state": "unavailable", "semantics": "may", "issues": ["snapshot_changed"], "omitted": 0}`
in that state.

Algorithm views (sliced output, steps involving a parameter or backward from a step):

```python
# Forward slice: steps that use or are influenced by historyHandling
result = wsi.query_view("HTML#navigate", involving="historyHandling")
print(result["content"])                # sliced algorithm markdown
for step in result["slice"]["steps"]:
    print(step["path"], step["role"])   # e.g. "13" "match"

# Backward slice: where does historyEntry in step 24.9.1 come from?
result = wsi.query_view("HTML#navigate", feeding="24.9.1:historyEntry")
print(result["content"])
print(result["slice"]["unfollowed"])    # opaque and loop-bound variables not followed
print(result["slice"]["rebound"])       # names let-bound at more than one step
```

`query_view(spec_anchor, involving=None, feeding=None, steps=None, depth=None, links=None, no_notes=None)`
returns a dict with a `content` key (sliced algorithm markdown with omission markers) and a `slice`
key containing: `algorithm`, `view`, `variables`, `steps`, `omitted`, `stores`, `unfollowed`,
`rebound`, `inputs`/`later_definitions` (backward slices), and `status` (with a `counts` sub-object).
`involving` accepts a name or list of names; `feeding` accepts `"24.9.1"` or `"24.9.1:historyEntry"`.
Raises `WebspecError` with a code starting `slice_` (e.g. `slice_unavailable`) when the slice index
is not available or the selector is invalid.

Errors are raised as `webspec_index.WebspecError`.

## Development

This project uses [`uv`](https://docs.astral.sh/uv/) and
[`maturin`](https://www.maturin.rs/).

```bash
# from bindings/python/
uv sync --extra test     # build the extension + install test deps
uv run pytest            # run the test suite
```
