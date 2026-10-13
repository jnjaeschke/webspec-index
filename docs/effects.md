# Possible effects

An effect says that an algorithm **may** perform an operation, including through another algorithm or a callback. It does not prove that a branch runs, that a path is feasible, or that an operation happens a particular number of times. An empty partial result cannot establish absence.

```sh
webspec-index query "HTML#navigate"
webspec-index effects "HTML#navigate" --compact --format markdown
webspec-index effects "HTML#navigate" --kind event.fire
webspec-index effects "HTML#navigate" --step 8
webspec-index effects "HTML#navigate" --summary-only
```

Queries include up to 12 groups, with omitted-group counts and coverage. `effects --compact` emits that same short preview without the specification content or witness traces. `effects --summary-only` lists all detected effects without traces; plain `effects` includes bounded traces. `--format markdown` selects the grouped human-readable rendering; the CLI default remains JSON. `effects` returns the full selected groups and bounded witnesses. Execution alternatives are `inline`, `separate`, and `unknown`, relative to the selected algorithm or step. A scheduling operation itself can run inline while its callback runs separately. Multiple occurrences with the same kind and parameters share a compact group; the detailed API retains their source sites.

Markdown summaries group related effects. Scheduling effects list their detection sites; event names appear under “May fire events” or “May dispatch events.” Author-code invocation mechanisms share one heading but retain their distinguishing labels. For example (illustrative source identifiers):

```text
- May queue a microtask in
  - HTML#foo:32.1
  - HTML#bar:1
- May fire events
  - load in HTML#foo:5
  - hashchange in HTML#bar:6
- May dispatch events
  - navigate in HTML#baz:27
```

Each effect shows up to three detection sites, including sites in transitively called algorithms, followed by a count of unshown sites. Numbered steps are preferred over unnumbered declarations, then locations are selected in canonical spec/anchor/step order. Declarations without a numbered step show only their section. These locations are endpoints, not full routes from the queried algorithm.

The shared API retains an optional primary `location` (`spec`, `anchor`, `url`, optional `step_path`), and adds up to two `other_locations`. `additional_locations` counts all sites besides the primary, including those in `other_locations`. All are persisted with summaries; cached display does not search witnesses. Effect IDs and execution alternatives remain in JSON and detailed witnesses. Event labels retain explicit “may fire X event” / “may dispatch X event” wording.

The query's existing 12-effect limit applies before presentation grouping. Truncated summaries report the total and offer commands for the full list (`effects --summary-only`) and traces with conditions (`effects`). Counts refer to distinct effects, including event names and invocation mechanisms, rather than top-level headings or source sites. Partial analysis is separate: additional effects may remain undetected. Trace searches are bounded; use `--kind scheduling.enqueue` or `--category events` to focus the request, and `--limit` to request more witnesses per effect.

Detailed Markdown initially collapses each effect beneath its human-readable title and stable effect ID; expanding it shows one numbered list per path. Repeated structural representations of a step are collapsed; distinct calls and scheduling boundaries remain visible. Code, variables, and source links retain Markdown formatting inside quoted excerpts. Reviewed declarations show their rationale and source endpoint rather than the algorithm's entire text. Links whose call syntax was not recognized, missing condition details, and preceding exits are labeled explicitly. A linked target can be known even when the source sentence does not establish an invocation; the analyzer follows such links conservatively.

The default `--limit 1` selects one representative path per effect; it is an output preference, not an analysis failure. Use `--limit N` to show more source sites or execution alternatives. The search indexes stored derivations and runs a backward breadth-first traversal from known effect sites, visiting each effect state once and sharing path suffixes. It chooses shortest paths with deterministic source-order ties, preserving occurrence identity, execution modes, call-site bodies, and source context. It does not enumerate every route through cycles or converging branches.

The default safety bounds are 20,000 graph edges (`--max-depth 20000`) and 20,000 distinct states shared across selected effects (`--max-witness-states 20000`). Graph edges include internal containment edges, so the depth limit is not the number of displayed calls. Reaching a trace limit does not remove an already detected effect. Diagnostics identify genuine search exhaustion; reaching the requested number of examples does not produce a warning. An effect with no complete displayed path includes known source sites and a focused retry command. For example:

```sh
webspec-index effects "HTML#navigate" --effect-id ef_7185cecba7f35a16 --max-depth 64 --max-witness-states 100000 --limit 3 --format markdown
```

“Continue the remaining steps in the current flow” describes one continuation edge, not the timing of the entire algorithm. Separately scheduled bodies are labeled at the boundary. All paths remain possible paths with unchecked feasibility.

`query --effects cached` returns only valid stored analysis. `query --effects off` preserves the original query shape. When a spec's content changed since the last publication, an ordinary query rebuilds effects inline within a time budget (default 5 s, overridden by `WEBSPEC_EFFECTS_INLINE_BUDGET_MS`). If the rebuild exceeds the budget or another rebuild is running, the result carries `"effects_status": {"state": "unavailable", "semantics": "may", "issues": ["snapshot_changed"], "omitted": 0}` and a note to run `webspec-index effects --all`. `update` refreshes effects and prepares representative paths after its indexing batch; `update --effects off` skips that work. Missing dependencies are reported, without recursively fetching them.

## Rules

The bundled rules live in `data/semantics/` and are embedded in the binary. Additional directories are merged with it using repeated `--rules PATH`; conflicting IDs and incompatible effect definitions are errors.

State YAML lives under `state/` in the same package. A package is partitioned by its first path component: files under `state/` are loaded by the state layer, every other file by the effects loader. `--rules PATH` applies to both layers; a query-time `--rules` package passed to `state` may contain only `rules` (no `types` or `fields`). See [docs/state.md](state.md) for the state rule schema.

This complete example adds a caller-scoped rule to the existing event vocabulary:

```yaml
schema: 1
package: project-events
rules:
  - id: navigation-dispatch
    description: Reviewed navigate event dispatch site.
    match:
      subject: HTML#inner-navigate-event-firing-algorithm
      anchor: DOM#concept-event-dispatch
      text: '(?i)dispatching\s+\*event\*\s+at\s+\*navigation\*'
    emit:
      kind: event.dispatch
      params:
        name: navigate
```

An anchor-only rule also declares the endpoint's intrinsic effect. A text rule is matched against an individual executable segment. Variables remain distinct from literals; unresolved arguments remain null. Supported captures, continuation declarations, reviewed summaries and environment mappings are defined by the [catalog schema](../schemas/effects/catalog.schema.json). The [contract reference](../schemas/effects/README.md) describes validation and the shared wire format. Rules cannot execute code.

A new file under `data/semantics/` must also be listed in `FILES` in `src/semantics/bundled.rs`; a unit test fails until it is. `tests/semantics_cases.rs` runs the acceptance cases in `tests/fixtures/semantics/cases/` against the bundled catalog: each case parses spec excerpts from `tests/fixtures/semantics/sources/` (provenance in its `manifest.json`), analyzes one subject, and compares effects, witnesses and issues. Rebuild to change the bundled catalog; use `--rules` for additional packages without rebuilding. Adding endpoints uses configuration; adding an unsupported control-flow form requires parser/engine work.

## Mention classification

The state IR exposes algorithm calls and link roles through `state <ALGORITHM> --calls`; effects still uses its verb rule until sub-project 4 replaces it with the IR's roles.

A link in algorithm prose is classified as an invocation candidate or a concept mention. The engine combines two signals:

1. **Target section type.** If the link target resolves to an algorithm section in the index (a structural algorithm body), the link is eligible for invocation. If the target resolves to a non-algorithm section (a definition, heading, or IDL anchor registered as an indexed anchor without a structural body), the link is a concept reference by default.

2. **Call-site lexical shape.** The `operation_relation` function checks whether the link's visible text or preceding context contains an invocation verb (navigate, fetch, fire, queue, run, perform, etc.). A verb match produces an `Invoke` relationship; no verb match produces `CandidateInvoke`.

The combined rule:

- **Algorithm target + verb match** → `Invoke` (inline execution).
- **Algorithm target + no verb match** → `CandidateInvoke` + `unresolved_invocation` issue.
- **Non-algorithm target + verb match** → `Invoke` (the verb overrides the target type; the link may reference an algorithm-like operation defined as a prose definition rather than a structural algorithm).
- **Non-algorithm target + no verb match** → `Mention`. No `unresolved_invocation` issue. The link is a concept reference (e.g., "the navigable's origin") and does not represent a call.
- **Unknown target (missing spec or anchor)** → `missing_spec` or `missing_anchor` issue, unchanged.

Mention edges do not propagate effects or issues during the fixed-point analysis.

## Clients and limits

Rust exposes `effects::{get_effect_summary, explain_effects, recompute_effects}`. Python exports the same functions with typed dictionaries; see the [binding examples](../bindings/python/README.md). LSP clients use `webspec/effects` and `webspec/effectExplanation`. VS Code shows per-step effect CodeLens rows. Clicking a category opens a native hover with collapsed effect headlines; clicking a headline toggles its prepared details without another server request. The **Show Spec Effects** command opens the ordinary hover at the cursor. All adapters consume the same analysis and snapshot identities.

Definition-only bodies have separate summaries and do not affect the defining step until invoked. Unsupported structures, unresolved calls and incomplete inputs remain explicit issues. PR effects, path-feasibility proofs, must-run-before assertions, and general validation of spec assertions are outside this release. Witness context is bounded and descriptive.

The [effects benchmark](../examples/benchmark_effects.rs) measures a supplied corpus without fetching specifications or changing the normal index. The [path benchmark](../examples/benchmark_effect_paths.rs) measures explanation of a saved analysis artifact. Warm queries read materialized summaries; they do not parse source HTML, scan regex rules, or propagate the graph. `scripts/bench.py` covers the full incremental path: a first query after an HTML change (inline re-parse and rebuild), the subsequent cached query (≤20 ms), `effects --all` over the changed corpus (≤2.5 s / 500 MB), and `effects --all --rebuild` from scratch (≤5 s / 1 GB).

Scheduling labels retain HTML terminology: “queue a task,” “queue a microtask,” and “enqueue steps on the session history traversal queue.” The compact machine key `queue: traversal` remains stable; it is expanded for human-facing output rather than displayed as an unexplained queue name.

The built-in scheduling rules cover direct, global, element, and media-element tasks; microtasks; parallel-queue enqueueing; both session-history traversal append operations; and MessagePort's direct task insertion. Task effects preserve an explicit source in the nullable `source` parameter, for example `{"queue":"task","source":"navigation and traversal task source"}`, rendered as “may queue a task on the navigation and traversal task source.” The catalog recognizes all 17 named HTML task sources in the reviewed snapshot. A dynamically supplied source remains null. Task sources are associated with task queues; they are not separate queue types.

`HTML#in-parallel` has its own effect, and reviewed promise scheduling includes `WEBIDL#wait-for-all` plus a web mapping for `HostEnqueuePromiseJob`. Higher-level wrappers are followed through their primitive calls. Data queues and custom-element reaction queues are not unconditional async endpoints: reaction processing can be synchronous, so analysis follows the actual microtask branch. The complete HTML inventory is `data/semantics/effects/20-scheduling.yaml`; the `scheduling-*` acceptance cases cover each primitive and named task source. Generated commands quote section selectors, including ordinary `"HTML#navigate"` selectors.
