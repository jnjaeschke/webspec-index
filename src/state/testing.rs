//! Offline helpers for tests and examples: extract (and later index) spec HTML
//! without network, fetch state or update checks.
use crate::state::ir::{Call, Expr, Statement, StatementSource};
use crate::state::model::{Signature, StateSpec, TypeExpr};
use crate::state::{extract_state, StateCatalog, StateInputs};

pub fn base_url(spec: &str) -> &'static str {
    match spec {
        "DOM" => "https://dom.spec.whatwg.org/",
        "ECMA-262" => "https://tc39.es/ecma262/",
        _ => "https://html.spec.whatwg.org/",
    }
}

pub fn extract_with_catalog(html: &str, spec: &str, catalog: &StateCatalog) -> StateSpec {
    let base = base_url(spec);
    let document = scraper::Html::parse_document(html);
    let parsed = crate::parse::parse_spec(html, spec, base).expect("parse_spec");
    let structure =
        crate::parse::steps::extract_step_structure_from_document(&document, spec, base, "hash:t");
    extract_state(&StateInputs {
        document: &document,
        spec,
        base_url: base,
        snapshot_sha: "hash:t",
        structure: &structure,
        sections: &parsed.sections,
        idl_definitions: &parsed.idl_definitions,
        catalog,
        body_nodes: None,
    })
}

/// Grammar only: unit tests must not change when the bundled catalog grows.
pub fn extract_html(html: &str, spec: &str) -> StateSpec {
    extract_with_catalog(html, spec, &StateCatalog::default())
}

/// The spec §8.3 `add-to-field-collection` verb rule as a state catalog file.
pub const ADD_RULE_YAML: &str = r#"schema: 1
package: webspec-index
rules:
  - id: add-to-field-collection
    match:
      text: '(?:^|, then |; )Add (?P<operand>.+?) to (?P<target>(?:the |this )?\S.*?)(?:\.|,|;|$)'
      exclude_text: ['(?i)\badd \S+ to \*\w+\*(?:\.|$)']
    emit:
      kind: state.mutate
      params:
        op: append
        target: {capture: target, from: path}
        operand: {capture: operand, from: text}
"#;

pub const MINI: &str = r##"<pre><code class="idl">partial interface <dfn data-lt="" id="document">Document</dfn> {};</code></pre>
<h3 id="the-document-object">The Document object</h3>
<p>Each <code><a href="#document">Document</a></code> has an <dfn id="is-initial-about:blank">is initial <code>about:blank</code></dfn>, which is a boolean, initially false.</p>
<div data-algorithm=""><p>The <dfn id="document-open-steps">document open steps</dfn>, given a <var>document</var>, are as follows:</p><ol>
<li><p>Set <var>document</var>'s <a href="#is-initial-about:blank">is initial <code>about:blank</code></a> to false.</p></li>
<li><p>If <var>document</var>'s <a href="#is-initial-about:blank">is initial <code>about:blank</code></a> is true, then return.</p></li>
<li><p>Add <var>x</var> to <var>document</var>'s <a href="#is-initial-about:blank">is initial <code>about:blank</code></a>.</p></li></ol></div>"##;

/// A `Document` initializer whose entries are a `<dl>` (spec §12.2 fixture 8).
pub const DL_HTML: &str = r##"<pre><code class="idl">partial interface <dfn data-lt="" id="document">Document</dfn> {};</code></pre>
      <p>Each <code><a href="#document">Document</a></code> has an <dfn id="is-initial-about:blank">is initial <code>about:blank</code></dfn>, which is a boolean, initially false.</p>
      <div data-algorithm=""><p>To <dfn id="creating-a-new-browsing-context">create a new browsing context</dfn>:</p><ol>
      <li><p>Let <var>document</var> be a new <code><a href="#document">Document</a></code>, with:</p>
      <dl class="props"><dt><a href="#is-initial-about:blank">is initial <code>about:blank</code></a></dt><dd>true</dd></dl></li></ol></div>"##;

/// `Event` prose: method, getter and setter steps plus callouts (spec §12.2
/// fixture 13).
pub const PROSE_DOM: &str = r##"<pre class="idl">interface <dfn data-dfn-type="interface" id="event">Event</dfn> {};</pre>
<p>An <dfn data-dfn-type="dfn" id="concept-event">event</dfn>.</p>
<p>Each <a href="#concept-event">event</a> has the following associated flags that are all initially unset:</p>
<ul><li><dfn data-dfn-for="Event" data-dfn-type="dfn" id="stop-propagation-flag">stop propagation flag</dfn></li></ul>
<h3 id="interface-event">Interface Event</h3>
<p>The <dfn data-dfn-for="Event" data-dfn-type="method" id="dom-event-stoppropagation"><code>stopPropagation()</code></dfn> method steps are to set <a href="https://webidl.spec.whatwg.org/#this">this</a>’s <a href="#stop-propagation-flag">stop propagation flag</a>.</p>
<p>The <a href="#dom-event-cancelbubble"><code>cancelBubble</code></a> getter steps are to return true if <a href="https://webidl.spec.whatwg.org/#this">this</a>’s <a href="#stop-propagation-flag">stop propagation flag</a> is set; otherwise false.</p>
<p>The <a href="#dom-event-cancelbubble"><code>cancelBubble</code></a> setter steps are to set <a href="https://webidl.spec.whatwg.org/#this">this</a>’s <a href="#stop-propagation-flag">stop propagation flag</a> if the given value is true; otherwise do nothing.</p>
<div class="note"><p>Set the <a href="#stop-propagation-flag">stop propagation flag</a> only from author code.</p></div>
<dl class="domintro"><dt>x</dt><dd><p>Set the <a href="#stop-propagation-flag">stop propagation flag</a>.</p></dd></dl>"##;

/// Parse, extract and store one spec snapshot without network or fetch state.
pub fn index_offline_with(
    conn: &rusqlite::Connection,
    spec: &str,
    base_url: &str,
    html: &str,
    catalog: &StateCatalog,
) -> anyhow::Result<i64> {
    let parsed = crate::parse::parse_spec(html, spec, base_url)?;
    let document = scraper::Html::parse_document(html);
    let sha = format!("hash:{spec}-offline");
    let structure =
        crate::parse::steps::extract_step_structure_from_document(&document, spec, base_url, &sha);
    let state = extract_state(&StateInputs {
        document: &document,
        spec,
        base_url,
        snapshot_sha: &sha,
        structure: &structure,
        sections: &parsed.sections,
        idl_definitions: &parsed.idl_definitions,
        catalog,
        body_nodes: None,
    });
    let spec_id = crate::db::write::insert_or_get_spec(conn, spec, base_url, "offline")?;
    let snapshot = crate::db::write::insert_snapshot(conn, spec_id, &sha, "2026-09-25")?;
    crate::db::write::insert_sections_bulk(conn, snapshot, &parsed.sections)?;
    crate::db::state::store_state(conn, snapshot, &state)?;
    crate::db::state::store_slice_indexes(
        conn,
        snapshot,
        &crate::state::slice::build_slice_indexes(&structure, &state),
    )?;
    Ok(snapshot)
}

pub const QUERY_DOM: &str = r##"<pre class="idl">interface <dfn data-dfn-type="interface" id="interface-node">Node</dfn> {};
interface <dfn data-dfn-type="interface" id="interface-element">Element</dfn> : Node {};
interface <dfn data-dfn-type="interface" id="interface-document">Document</dfn> : Node {};</pre>
<p>A <dfn data-dfn-type="dfn" id="concept-node">node</dfn>. A <dfn data-dfn-type="dfn" id="concept-document">document</dfn>.</p>
<h3 id="interface-node-h">Node</h3>
<p>Each <a href="#concept-node">node</a> has an associated <dfn data-dfn-for="Node" data-dfn-type="dfn" id="concept-node-document">node document</dfn>, set upon creation, that is a <a href="#concept-document">document</a>.</p>
<div class="algorithm"><p>To <dfn id="concept-node-adopt">adopt</dfn> a <var>node</var> into a <var>document</var>:</p><ol>
<li><p>Set <var>node</var>’s <a href="#concept-node-document">node document</a> to <var>document</var>.</p></li></ol></div>"##;

pub const QUERY_HTML: &str = r##"<pre><code class="idl">partial interface <dfn data-lt="" id="document">Document</dfn> {};</code></pre>
<h3 id="the-document-object">The Document object</h3>
<p>Each <code><a href="#document">Document</a></code> has an <dfn id="is-initial-about:blank">is initial <code>about:blank</code></dfn>, which is a boolean, initially false.</p>
<div data-algorithm=""><p>The <dfn id="document-open-steps">document open steps</dfn> are:</p><ol>
<li><p>Set <var>document</var>'s <a href="#is-initial-about:blank">is initial <code>about:blank</code></a> to false.</p></li>
<li><p>Set <var>d</var>'s <a href="https://dom.spec.whatwg.org/#concept-node-document">node document</a> to <var>x</var>.</p></li>
<li><p>Add <var>s</var> to <var>s</var>'s <a href="https://dom.spec.whatwg.org/#concept-node-document">node document</a>'s <a href="#open-dialogs-list">open dialogs list</a>.</p></li></ol></div>
<h3 id="sandboxing">Sandboxing</h3>
<p>A <dfn id="sandboxing-flag-set">sandboxing flag set</dfn> is a set of zero or more of the following flags, which are used to restrict abilities:</p>
<dl><dt>The <dfn id="sandboxed-navigation-browsing-context-flag">sandboxed navigation browsing context flag</dfn></dt><dd><p>This flag prevents content from navigating.</p></dd></dl>
<div data-algorithm=""><p>To <dfn id="parse-a-sandboxing-directive">parse a sandboxing directive</dfn> into <var>output</var>:</p><ol>
<li><p>Set <var>output</var>'s <a href="#sandboxed-navigation-browsing-context-flag">sandboxed navigation browsing context flag</a>.</p></li>
<li><p>Unset <var>output</var>'s <a href="#sandboxed-navigation-browsing-context-flag">sandboxed navigation browsing context flag</a>.</p></li></ol></div>"##;

/// In-memory DB holding the given specs, indexed offline with the grammar only
/// (empty catalog), so query tests do not change when the bundled catalog grows.
pub fn db_with(specs: &[(&str, &str)]) -> rusqlite::Connection {
    let conn = crate::db::open_in_memory().expect("in-memory db");
    for (spec, html) in specs {
        index_offline_with(&conn, spec, base_url(spec), html, &StateCatalog::default())
            .expect("index_offline");
    }
    conn
}

/// Structure, state (empty catalog) and slice indexes of one HTML string.
pub fn slice_indexes_html(html: &str, spec: &str) -> Vec<crate::state::slice::SliceIndex> {
    let structure =
        crate::parse::steps::extract_step_structure(html, spec, base_url(spec), "hash:t");
    let state = extract_html(html, spec);
    crate::state::slice::build_slice_indexes(&structure, &state)
}

/// DOM and HTML query fixtures together.
pub fn query_fixture_db() -> rusqlite::Connection {
    db_with(&[("DOM", QUERY_DOM), ("HTML", QUERY_HTML)])
}

/// Parse, extract and store one spec snapshot using the bundled catalog.
pub fn index_offline(
    conn: &rusqlite::Connection,
    spec: &str,
    base_url: &str,
    html: &str,
) -> anyhow::Result<i64> {
    index_offline_with(
        conn,
        spec,
        base_url,
        html,
        crate::state::extract::bundled_catalog(),
    )
}

/// Golden-set site key: `SPEC#subject:step` when there is a step path,
/// `SPEC#subject (role)` for a prose role other than `normative`, else `SPEC#subject`.
pub fn site_key(spec: &str, subject: &str, step_path: Option<&str>, role: Option<&str>) -> String {
    match (step_path, role) {
        (Some(step), _) => format!("{spec}#{subject}:{step}"),
        (None, Some(role)) if role != "normative" => format!("{spec}#{subject} ({role})"),
        _ => format!("{spec}#{subject}"),
    }
}

/// Hand-built slice index for query-time tests. `steps`: `(path, mentioned names)` in document
/// order; `edges`: `(step path, kind, defined or root name, used names)`; `loops`: `(step path,
/// loop-bound name)`. Names are interned in first-mention order over steps, then edges, then loops;
/// `mentions` and `uses` are sorted var indexes, as `build_slice_indexes` produces them.
/// `Store` edges get the target `*{name}*'s field`.
pub fn slice_index(
    anchor: &str,
    steps: &[(&str, &[&str])],
    edges: &[(&str, crate::state::slice::DefKind, Option<&str>, &[&str])],
    loops: &[(&str, &str)],
) -> crate::state::slice::SliceIndex {
    use crate::state::slice::index::derive_parents;
    use crate::state::slice::{DefEdge, DefKind, SliceIndex, SliceStep};

    fn intern(vars: &mut Vec<String>, name: &str) -> u32 {
        if let Some(pos) = vars.iter().position(|v| v == name) {
            return pos as u32;
        }
        vars.push(name.to_owned());
        (vars.len() - 1) as u32
    }

    let mut vars: Vec<String> = Vec::new();

    let mut slice_steps: Vec<SliceStep> = Vec::with_capacity(steps.len());
    for (path, names) in steps {
        let mut mentions: Vec<u32> = names.iter().map(|n| intern(&mut vars, n)).collect();
        mentions.sort_unstable();
        mentions.dedup();
        slice_steps.push(SliceStep {
            path: path.to_string(),
            parent: None,
            mentions,
            loop_binds: vec![],
        });
    }

    let paths: Vec<String> = slice_steps.iter().map(|s| s.path.clone()).collect();
    let parents = derive_parents(&paths);
    for (step, parent) in slice_steps.iter_mut().zip(parents) {
        step.parent = parent;
    }

    let step_paths: Vec<&str> = slice_steps.iter().map(|s| s.path.as_str()).collect();
    let find_step = |path: &str| -> usize {
        step_paths
            .iter()
            .position(|p| *p == path)
            .expect("step path not found")
    };

    let mut slice_edges: Vec<DefEdge> = Vec::with_capacity(edges.len());
    for (step_path, kind, name, used_names) in edges {
        let step = find_step(step_path) as u32;
        let var = name.map(|n| intern(&mut vars, n));
        let mut uses: Vec<u32> = used_names.iter().map(|n| intern(&mut vars, n)).collect();
        uses.sort_unstable();
        uses.dedup();
        let target = if *kind == DefKind::Store {
            name.map(|n| format!("*{n}*'s field"))
        } else {
            None
        };
        slice_edges.push(DefEdge {
            step,
            kind: *kind,
            var,
            uses,
            target,
        });
    }

    let loop_assignments: Vec<(usize, &str)> = loops
        .iter()
        .map(|(path, name)| (find_step(path), *name))
        .collect();
    drop(step_paths);
    for (idx, loop_name) in loop_assignments {
        let var_idx = intern(&mut vars, loop_name);
        slice_steps[idx].loop_binds.push(var_idx);
    }

    SliceIndex {
        anchor: anchor.to_owned(),
        vars,
        steps: slice_steps,
        edges: slice_edges,
    }
}

/// A reduced DOM "insert": 18 steps, Let chains, a store and a loop (spec §12 fixture G1).
pub fn slice_fixture_insert() -> crate::state::slice::SliceIndex {
    slice_index(
        "insert",
        &[
            ("1", &["nodes", "node"]),
            ("2", &["count", "nodes"]),
            ("3", &["count"]),
            ("4", &["node"]),
            ("4.1", &[]),
            ("4.2", &["node", "nodes"]),
            ("5", &["child"]),
            ("5.1", &["parent", "child", "count"]),
            ("5.2", &["parent", "child", "count"]),
            ("6", &["previousSibling", "child", "parent"]),
            ("7", &["node", "nodes"]),
            ("7.1", &["node", "parent"]),
            ("7.2", &["node", "parent"]),
            ("7.3", &["node", "inclusiveDescendant"]),
            ("7.3.1", &["inclusiveDescendant"]),
            (
                "8",
                &[
                    "suppressObservers",
                    "parent",
                    "nodes",
                    "previousSibling",
                    "child",
                ],
            ),
            ("9", &["parent"]),
            ("10", &["staticNodeList"]),
        ],
        &[
            (
                "1",
                crate::state::slice::DefKind::Let,
                Some("nodes"),
                &["node"],
            ),
            (
                "2",
                crate::state::slice::DefKind::Let,
                Some("count"),
                &["nodes"],
            ),
            (
                "6",
                crate::state::slice::DefKind::Let,
                Some("previousSibling"),
                &["child", "parent"],
            ),
            (
                "7.2",
                crate::state::slice::DefKind::Store,
                Some("parent"),
                &["node"],
            ),
        ],
        &[("7.3", "inclusiveDescendant")],
    )
}

/// A reduced navigate: 11 steps, backward-slice fixture (spec §12 fixture G3).
pub fn slice_fixture_navigate() -> crate::state::slice::SliceIndex {
    slice_index(
        "navigate",
        &[
            ("1", &["documentResource"]),
            ("2", &["initiator", "sourceDocument"]),
            ("3", &["sourceDocument"]),
            ("3.1", &["initiator", "navigable"]),
            ("4", &["state", "referrerPolicy", "initiator"]),
            ("4.1", &["state", "initiator"]),
            ("5", &["entry", "url", "state"]),
            ("6", &["navigable"]),
            ("7", &["navigable", "entry", "historyHandling"]),
            ("8", &["entry"]),
            ("9", &["other"]),
        ],
        &[
            (
                "2",
                crate::state::slice::DefKind::Let,
                Some("initiator"),
                &["sourceDocument"],
            ),
            (
                "3.1",
                crate::state::slice::DefKind::Set,
                Some("initiator"),
                &["navigable"],
            ),
            (
                "4",
                crate::state::slice::DefKind::Let,
                Some("state"),
                &["referrerPolicy", "initiator"],
            ),
            (
                "4",
                crate::state::slice::DefKind::Opaque,
                None,
                &["initiator"],
            ),
            (
                "4.1",
                crate::state::slice::DefKind::Store,
                Some("state"),
                &["initiator"],
            ),
            (
                "5",
                crate::state::slice::DefKind::Let,
                Some("entry"),
                &["url", "state"],
            ),
            (
                "6",
                crate::state::slice::DefKind::Store,
                Some("navigable"),
                &[],
            ),
            ("8", crate::state::slice::DefKind::Set, Some("entry"), &[]),
        ],
        &[],
    )
}

/// Classes (snake_case) of the occurrences of `field_selector` (`SPEC#anchor`) in the
/// sources whose [`site_key`] is `site`, across every current snapshot's state model.
pub fn occurrence_classes(
    conn: &rusqlite::Connection,
    field_selector: &str,
    site: &str,
) -> std::collections::BTreeSet<String> {
    use crate::state::extract::{class_name, role_name};
    use crate::state::ir::SourceContext;
    let (field_spec, field_anchor) = field_selector
        .split_once('#')
        .expect("field selector is SPEC#anchor");
    let mut classes = std::collections::BTreeSet::new();
    for snapshot in crate::db::state::state_snapshots(conn).expect("state snapshots") {
        let Some(state) =
            crate::db::state::load_state_model(conn, snapshot.id).expect("state model")
        else {
            continue;
        };
        let sources: std::collections::HashSet<&str> = state
            .sources
            .iter()
            .filter(|source| {
                let (step_path, role) = match &source.context {
                    SourceContext::Algorithm { step_path, .. } => (step_path.as_deref(), None),
                    SourceContext::BranchLabel { step_path, .. } => {
                        (Some(step_path.as_str()), None)
                    }
                    SourceContext::Prose {
                        step_path, role, ..
                    } => (step_path.as_deref(), Some(role_name(*role))),
                    SourceContext::Intro { .. } => (None, None),
                };
                site_key(&state.spec, &source.subject.anchor, step_path, role) == site
            })
            .map(|source| source.id.as_str())
            .collect();
        for occurrence in &state.occurrences {
            let targets_field = occurrence
                .target
                .as_ref()
                .is_some_and(|t| t.spec == field_spec && t.anchor == field_anchor);
            if targets_field && sources.contains(occurrence.source_id.as_str()) {
                classes.insert(class_name(occurrence.class).to_owned());
            }
        }
    }
    classes
}

/// Compact type notation used by tests and golden expectations.
pub fn ty(t: &TypeExpr) -> String {
    use crate::state::model::{InfraKind, TypeRef};
    match t {
        TypeExpr::Unknown => "?".into(),
        TypeExpr::Primitive(p) => serde_json::to_value(p)
            .unwrap()
            .as_str()
            .unwrap()
            .replace('_', " "),
        TypeExpr::Nominal {
            ty: TypeRef::Known(key),
            ..
        } => key.to_string(),
        TypeExpr::Nominal {
            ty: TypeRef::Unresolved(target),
            ..
        } => {
            format!("{}#{}", target.spec, target.anchor)
        }
        TypeExpr::Infra { kind, args } => {
            let name = match kind {
                InfraKind::List => "list",
                InfraKind::OrderedSet => "ordered set",
                InfraKind::OrderedMap => "ordered map",
                InfraKind::Map => "map",
                InfraKind::Tuple => "tuple",
                InfraKind::Struct => "struct",
            };
            if args.is_empty() {
                name.into()
            } else {
                format!(
                    "{name}<{}>",
                    args.iter().map(ty).collect::<Vec<_>>().join(", ")
                )
            }
        }
        TypeExpr::Union(items) => items.iter().map(ty).collect::<Vec<_>>().join(" | "),
        TypeExpr::Null => "null".into(),
        TypeExpr::Enumerated(values) => values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(" | "),
        TypeExpr::Opaque { text } => format!("opaque({text})"),
        TypeExpr::Idl { text } => format!("idl-type:{text}"),
    }
}

pub fn var(name: &str) -> Expr {
    Expr::Var(name.to_string())
}

/// The signature of `anchor`; panics listing the anchors that have one.
pub fn signature<'a>(state: &'a StateSpec, anchor: &str) -> &'a Signature {
    state
        .signatures
        .iter()
        .find(|s| s.algorithm.anchor == anchor)
        .unwrap_or_else(|| {
            panic!(
                "no signature for {anchor}; have {:?}",
                state
                    .signatures
                    .iter()
                    .map(|s| &s.algorithm.anchor)
                    .collect::<Vec<_>>()
            )
        })
}

fn sources_at<'a>(state: &'a StateSpec, anchor: &str, step_path: &str) -> Vec<&'a StatementSource> {
    use crate::state::ir::SourceContext;
    state
        .sources
        .iter()
        .filter(|s| {
            s.subject.anchor == anchor
                && match &s.context {
                    SourceContext::Algorithm {
                        step_path: Some(p), ..
                    }
                    | SourceContext::Prose {
                        step_path: Some(p), ..
                    } => p == step_path,
                    SourceContext::BranchLabel { step_path: p, .. } => p == step_path,
                    _ => false,
                }
        })
        .collect()
}

/// Statements of the sources of `anchor`'s step `step_path`, in source then span order.
pub fn statements_at<'a>(
    state: &'a StateSpec,
    anchor: &str,
    step_path: &str,
) -> Vec<&'a Statement> {
    let ids: Vec<&str> = sources_at(state, anchor, step_path)
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    let mut out: Vec<&Statement> = state
        .statements
        .iter()
        .filter(|s| ids.contains(&s.source_id.as_str()))
        .collect();
    out.sort_by_key(|s| (ids.iter().position(|id| *id == s.source_id), s.span.start));
    out
}

pub fn calls_at<'a>(state: &'a StateSpec, anchor: &str, step_path: &str) -> Vec<&'a Call> {
    let ids: Vec<&str> = sources_at(state, anchor, step_path)
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    let mut out: Vec<&Call> = state
        .calls
        .iter()
        .filter(|c| ids.contains(&c.source_id.as_str()))
        .collect();
    out.sort_by_key(|c| (ids.iter().position(|id| *id == c.source_id), c.span.start));
    out
}

pub fn source_at<'a>(state: &'a StateSpec, anchor: &str, step_path: &str) -> &'a StatementSource {
    sources_at(state, anchor, step_path)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no source for {anchor}:{step_path}"))
}

pub const NAV_HTML: &str = r##"<div data-algorithm=""><p>To <dfn id="navigate">navigate</dfn> a <a href="#navigable">navigable</a> <var>navigable</var> to a <a href="https://url.spec.whatwg.org/#concept-url">URL</a> <var>url</var> using an optional <code><a href="https://dom.spec.whatwg.org/#document">Document</a></code>-or-null <var id="source-browsing-context">sourceDocument</var> (default null), with an optional boolean <dfn data-dfn-for="navigate" id="exceptions-enabled"><var>exceptionsEnabled</var></dfn> (default false), an optional <code><a href="#navigationhistorybehavior">NavigationHistoryBehavior</a></code> <dfn data-dfn-for="navigate" id="navigation-hh"><var>historyHandling</var></dfn> (default "<code><a href="#navigationhistorybehavior-auto">auto</a></code>"), and an optional <a href="#referrer-policy">referrer policy</a> <dfn id="navigation-referrer-policy"><var>referrerPolicy</var></dfn> (default the empty string):</p>
<ol><li><p>Let <var>cspNavigationType</var> be "<code>form-submission</code>" if <var>exceptionsEnabled</var> is true; otherwise "<code>other</code>".</p></li>
<li><p>If <var>url</var> is failure, then return.</p></li>
<li><p><a href="#in-parallel">In parallel</a>:</p><ol><li><p>Set <var>navigable</var>'s <a href="#ongoing-navigation">ongoing navigation</a> to null.</p></li></ol></li></ol></div>
<div data-algorithm=""><p>To <dfn id="location-object-navigate"><code>Location</code>-object navigate</dfn> a <code><a href="#location">Location</a></code> object <var>location</var> to a <a href="https://url.spec.whatwg.org/#concept-url">URL</a> <var>url</var>, optionally given a <code><a href="#navigationhistorybehavior">NavigationHistoryBehavior</a></code> <var>historyHandling</var> (default "<code><a href="#navigationhistorybehavior-auto">auto</a></code>"):</p>
<ol><li><p>Let <var>navigable</var> be <var>location</var>'s <a href="#relevant-global-object">relevant global object</a>'s <a href="#window-bc">navigable</a>.</p></li>
<li><p>Let <var>sourceDocument</var> be the <a href="#incumbent-global-object">incumbent global object</a>'s <a href="#concept-document-window">associated <code>Document</code></a>.</p></li>
<li><p>If <var>location</var>'s <a href="#relevant-document">relevant <code>Document</code></a> is not yet <a href="#completely-loaded">completely loaded</a>, and the <a href="#incumbent-global-object">incumbent global object</a> does not have <a href="#transient-activation">transient activation</a>, then set <var>historyHandling</var> to "<code><a href="#navigationhistorybehavior-replace">replace</a></code>".</p></li>
<li><p><a href="#navigate">Navigate</a> <var>navigable</var> to <var>url</var> using <var>sourceDocument</var>, with <a href="#exceptions-enabled"><var>exceptionsEnabled</var></a> set to true and <a href="#navigation-hh"><var>historyHandling</var></a> set to <var>historyHandling</var>.</p></li></ol></div>
<div data-algorithm=""><p>The <dfn data-dfn-for="Location" id="dom-location-assign"><code>assign(<var>url</var>)</code></dfn> method steps are:</p>
<ol><li><p>Let <var>urlRecord</var> be the result of <a href="#encoding-parsing-a-url">encoding-parsing a URL</a> given <var>url</var>, relative to the <a href="#entry-settings-object">entry settings object</a>.</p></li>
<li><p><a href="#location-object-navigate"><code>Location</code>-object navigate</a> <a href="https://webidl.spec.whatwg.org/#this">this</a> to <var>urlRecord</var>.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="encoding-parsing-a-url">encoding-parse a URL</dfn> given a string <var>url</var>, relative to an <a href="#environment-settings-object">environment settings object</a> or <code><a href="https://dom.spec.whatwg.org/#document">Document</a></code> <var>environment</var>:</p><ol><li><p>Return <var>url</var>.</p></li></ol></div>"##;

pub const INSERT_DOM: &str = r##"<pre class="idl">interface <dfn data-dfn-type="interface" id="interface-node">Node</dfn> {};</pre>
<p>A <dfn data-dfn-type="dfn" id="concept-node">node</dfn> is a <code><a href="#interface-node">Node</a></code> object.</p>
<div class="algorithm"><p>To <dfn data-dfn-type="dfn" id="concept-node-insert">insert</dfn> a <a href="#concept-node">node</a> <var>node</var> into a <a href="#concept-node">node</a> <var>parent</var> before null or a <a href="#concept-node">node</a> <var>child</var>, with an optional boolean <dfn data-dfn-for="insert" data-dfn-type="dfn" id="insert-suppressobservers"><var>suppressObservers</var></dfn> (default false):</p>
<ol><li><p>Let <var>nodes</var> be <var>node</var>’s <a href="#concept-tree-child">children</a>, if <var>node</var> is a <code>DocumentFragment</code> node; otherwise « <var>node</var> ».</p></li>
<li><p>For each <var>node</var> of <var>nodes</var>, in <a href="#concept-tree-order">tree order</a>:</p><ol><li><p>Continue.</p></li></ol></li></ol></div>
<div class="algorithm"><p>To <dfn data-dfn-type="dfn" id="concept-node-replace">replace</dfn> a <a href="#concept-node">node</a> <var>child</var> with a <a href="#concept-node">node</a> <var>node</var> within a <a href="#concept-node">node</a> <var>parent</var>:</p>
<ol><li><p>Let <var>referenceChild</var> be <var>child</var>’s <a href="#concept-tree-next-sibling">next sibling</a>.</p></li>
<li><p><a href="#concept-node-insert">Insert</a> <var>node</var> into <var>parent</var> before <var>referenceChild</var> with <a href="#insert-suppressobservers">suppressObservers</a> set to true.</p></li></ol></div>"##;

pub const EVENT_DOM: &str = r##"<pre class="idl">interface <dfn data-dfn-type="interface" id="interface-event">Event</dfn> {
  undefined <a href="#dom-event-initevent">initEvent</a>(DOMString <dfn data-dfn-for="Event/initEvent(type, bubbles, cancelable)" data-dfn-type="argument" id="dom-event-initevent-type-type">type</dfn>, optional boolean <dfn data-dfn-for="Event/initEvent(type, bubbles, cancelable)" data-dfn-type="argument" id="dom-event-initevent-type-bubbles-bubbles">bubbles</dfn> = false, optional boolean <dfn data-dfn-for="Event/initEvent(type, bubbles, cancelable)" data-dfn-type="argument" id="dom-event-initevent-type-cancelable-cancelable">cancelable</dfn> = false); // legacy
  readonly attribute DOMString <a href="#dom-event-type">type</a>;
};</pre>
<p>Each <a href="#concept-event">event</a> has a <dfn data-dfn-for="Event" id="dispatch-flag">dispatch flag</dfn> and an <dfn data-dfn-for="Event" id="initialized-flag">initialized flag</dfn>.</p>
<p>An <dfn data-dfn-type="dfn" id="concept-event">event</dfn> is an <code><a href="#interface-event">Event</a></code> object.</p>
<div class="algorithm"><p>The <dfn data-dfn-for="Event" data-dfn-type="method" id="dom-event-initevent"><code>initEvent(<var>type</var>, <var>bubbles</var>, <var>cancelable</var>)</code></dfn> method steps are:</p>
<ol><li><p>If <a href="https://webidl.spec.whatwg.org/#this">this</a>’s <a href="#dispatch-flag">dispatch flag</a> is set, then return.</p></li>
<li><p><a href="#concept-event-initialize">Initialize</a> <a href="https://webidl.spec.whatwg.org/#this">this</a> with <var>type</var>, <var>bubbles</var>, and <var>cancelable</var>.</p></li></ol></div>
<div class="algorithm"><p>To <dfn data-dfn-for="Event" data-dfn-type="dfn" id="concept-event-initialize">initialize</dfn> an <var>event</var>, with <var>type</var>, <var>bubbles</var>, and <var>cancelable</var>, run these steps:</p>
<ol><li><p>Set <var>event</var>’s <a href="#initialized-flag">initialized flag</a>.</p></li></ol></div>
<p>The <dfn data-dfn-for="Event" data-dfn-type="attribute" id="dom-event-type"><code>type</code></dfn> getter steps are to return <a href="https://webidl.spec.whatwg.org/#this">this</a>’s <a href="#event-type">type</a>.</p>"##;

pub const FALLBACK_HTML: &str = r##"<pre><code class="idl">partial interface <dfn data-lt="" id="document">Document</dfn> {};</code></pre>
<p>Each <code><a href="#document">Document</a></code> has an <dfn id="concept-document-about-base-url">about base URL</dfn>, a <a href="https://url.spec.whatwg.org/#concept-url">URL</a> or null, initially null.</p>
<div data-algorithm=""><p>The <dfn id="fallback-base-url">fallback base URL</dfn> of a <code><a href="#document">Document</a></code> object <var>document</var> is the <a href="https://url.spec.whatwg.org/#concept-url">URL record</a> obtained by running these steps:</p>
<ol><li><p>If <var>document</var> is <a href="#an-iframe-srcdoc-document">an <code>iframe</code> <code>srcdoc</code> document</a>, then:</p><ol><li><p><a href="https://infra.spec.whatwg.org/#assert">Assert</a>: <var>document</var>'s <a href="#concept-document-about-base-url">about base URL</a> is non-null.</p></li><li><p>Return <var>document</var>'s <a href="#concept-document-about-base-url">about base URL</a>.</p></li></ol></li>
<li><p>Return <a href="#about:blank"><code>about:blank</code></a>.</p></li></ol></div>
<div data-algorithm=""><p>Two <a href="#concept-origin">origins</a>, <var>A</var> and <var>B</var>, are said to be <dfn id="same-origin">same origin</dfn> if the following algorithm returns true:</p>
<ol><li><p>If <var>A</var> and <var>B</var> are the same <a href="#concept-origin-opaque">opaque origin</a>, then return true.</p></li><li><p>Return false.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="use-the-base">use the base</dfn> given a <code><a href="#document">Document</a></code> <var>doc</var>:</p>
<ol><li><p>Let <var>u</var> be <var>doc</var>'s <a href="#fallback-base-url">fallback base URL</a>.</p></li>
<li><p>If <var>doc</var>'s <a href="#concept-document-origin">origin</a> and <var>u</var> are <a href="#same-origin">same origin</a>, then return <var>u</var>.</p></li>
<li><p>Assert: this is running on <var>traversable</var>'s <a href="#tn-session-history-traversal-queue">session history traversal queue</a>.</p></li>
<li><p>Assert: <var>element</var> is an <code>input</code> element whose <code>type</code> attribute is in the Color state.</p></li></ol></div>"##;

pub const PREPARE_EVENT_HTML: &str = r##"<div data-algorithm=""><p>When the steps below say to <dfn id="prepare-an-event">prepare an event</dfn> named <var>event</var> for a <a href="#text-track-cue">text track cue</a> <var>target</var> with a time <var>time</var>, the user agent must run these steps:</p><ol><li><p>Return.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="time-marches-on">time march on</dfn> given a <a href="#text-track-cue">text track cue</a> <var>cue</var>:</p>
<ol><li><p><a href="#prepare-an-event">Prepare an event</a> named <code>enter</code> for <var>cue</var> with the time <var>cue</var>'s <a href="#text-track-cue-start-time">start time</a>.</p></li></ol></div>"##;

pub const GIVENLIST_HTML: &str = r##"<div data-algorithm=""><p>The <dfn id="inner-navigate-event-firing-algorithm">inner <code>navigate</code> event firing algorithm</dfn> consists of the following steps, given a <code><a href="#navigationtype">NavigationType</a></code> <var>navigationType</var>, a <code><a href="#navigationdestination">NavigationDestination</a></code> <var>destination</var>, and a <a href="#user-navigation-involvement">user navigation involvement</a> <var>userInvolvement</var>:</p><ol><li><p>Return true.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="fire-a-traverse-navigate-event">fire a traverse <code>navigate</code> event</dfn> given a <var>destination</var> and a <var>userInvolvement</var>:</p>
<ol><li><p>Return the result of performing the <a href="#inner-navigate-event-firing-algorithm">inner <code>navigate</code> event firing algorithm</a> given "<code>traverse</code>", <var>destination</var>, and "<code>none</code>".</p></li>
<li><p>Return the result of performing the <a href="#inner-navigate-event-firing-algorithm">inner <code>navigate</code> event firing algorithm</a> with "<code>push</code>", <var>destination</var>, and <var>userInvolvement</var>.</p></li></ol></div>"##;

pub const CREATE_ELEMENT_DOM: &str = r##"<p>A <dfn data-dfn-type="dfn" id="concept-document">document</dfn> is a <code>Document</code> object.</p>
<div class="algorithm"><p>To <dfn data-dfn-type="dfn" id="concept-create-element">create an element</dfn>, given a <a href="#concept-document">document</a> <var>document</var>, string <var>localName</var>, string-or-null <var>namespace</var>, and optionally a string-or-null <var>prefix</var> (default null), string-or-null <var>is</var> (default null), and boolean <var>synchronousCustomElements</var> (default false):</p><ol><li><p>Let <var>result</var> be null.</p></li></ol></div>"##;

pub const AUTODIR_HTML: &str = r##"<pre><code class="idl">interface <dfn id="htmlelement">HTMLElement</dfn> : <a href="https://dom.spec.whatwg.org/#element">Element</a> {};</code></pre>
<div data-algorithm=""><p>To compute the <dfn id="auto-directionality">auto directionality</dfn> given an element <var>element</var>:</p><ol><li><p>Return null.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="use-objects">use objects</dfn> given an object implementing <a href="#canvasuserinterface">CanvasUserInterface</a> <var>ui</var>:</p><ol><li><p>Return.</p></li></ol></div>
<div data-algorithm=""><p>The <dfn id="rules-to-parse-a-date-string">rules to parse a date string</dfn> are as follows:</p><ol><li><p>Return.</p></li></ol></div>"##;

pub const ECMA_HTML: &str = r##"<emu-clause id="sec-stringindexof" type="abstract operation"><h1><span class="secnum">6.1.4.1</span> StringIndexOf ( <var>string</var>, <var>searchValue</var>, <var>fromIndex</var> )</h1><p>The abstract operation StringIndexOf takes arguments <var>string</var> (a String), <var>searchValue</var> (a String), and <var>fromIndex</var> (a non-negative <emu-xref href="#integer"><a href="#integer">integer</a></emu-xref>) and returns a non-negative <emu-xref href="#integer"><a href="#integer">integer</a></emu-xref> or <emu-const>not-found</emu-const>. It performs the following steps when called:</p><emu-alg><ol><li>Let <var>len</var> be the length of <var>string</var>.</li><li>Return <emu-const>not-found</emu-const>.</li></ol></emu-alg></emu-clause>
<emu-clause id="sec-caller" type="abstract operation"><h1><span class="secnum">6.1.4.2</span> Caller ( <var>s</var> )</h1><p>The abstract operation Caller takes argument <var>s</var> (a String) and returns an integer. It returns the index of "x" in <var>s</var>. It performs the following steps when called:</p><emu-alg><ol><li>Let <var>i</var> be ! <emu-xref aoid="StringIndexOf"><a href="#sec-stringindexof">StringIndexOf</a></emu-xref>(<var>s</var>, "x", 0).</li><li>Return <var>i</var>.</li></ol></emu-alg></emu-clause>"##;

pub const FIRE_DOM: &str = r##"<div class="algorithm"><p>To <dfn data-dfn-type="dfn" id="concept-event-fire">fire an event</dfn> named <var>e</var> at <var>target</var>, optionally using an <var>eventConstructor</var>:</p><ol><li><p>Return true.</p></li></ol></div>
<div class="algorithm"><p>To <dfn data-dfn-type="dfn" id="signal-change">signal a change</dfn> given an <var>element</var>:</p>
<ol><li><p><a href="#concept-event-fire">Fire an event</a> named <code>change</code> at <var>element</var>, with its <code>bubbles</code> attribute initialized to true, and return.</p></li>
<li><p><a href="#concept-event-fire">Fire an event</a> named <code>input</code> at <a href="#the-url-to-use">the element to use</a>.</p></li></ol></div>"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::steps::AnchorTarget;
    use crate::state::model::{InfraKind, Primitive, TypeKey, TypeRef};

    #[test]
    fn ty_renders_the_compact_notation() {
        let doc = TypeExpr::Nominal {
            ty: TypeRef::Known(TypeKey::Idl("Document".into())),
            text: "Document".into(),
        };
        assert_eq!(
            ty(&TypeExpr::Union(vec![doc.clone(), TypeExpr::Null])),
            "idl:Document | null"
        );
        assert_eq!(ty(&TypeExpr::Primitive(Primitive::Boolean)), "boolean");
        assert_eq!(
            ty(&TypeExpr::Idl {
                text: "DOMString".into()
            }),
            "idl-type:DOMString"
        );
        assert_eq!(
            ty(&TypeExpr::Infra {
                kind: InfraKind::List,
                args: vec![doc]
            }),
            "list<idl:Document>"
        );
        assert_eq!(
            ty(&TypeExpr::Opaque {
                text: "a String".into()
            }),
            "opaque(a String)"
        );
        assert_eq!(ty(&TypeExpr::Unknown), "?");
        let url = TypeExpr::Nominal {
            ty: TypeRef::Unresolved(AnchorTarget {
                spec: "URL".into(),
                anchor: "concept-url".into(),
            }),
            text: "URL".into(),
        };
        assert_eq!(ty(&url), "URL#concept-url");
    }

    #[test]
    fn every_fixture_parses_without_panicking() {
        for (html, spec) in [
            (NAV_HTML, "HTML"),
            (INSERT_DOM, "DOM"),
            (EVENT_DOM, "DOM"),
            (FALLBACK_HTML, "HTML"),
            (PREPARE_EVENT_HTML, "HTML"),
            (GIVENLIST_HTML, "HTML"),
            (CREATE_ELEMENT_DOM, "DOM"),
            (AUTODIR_HTML, "HTML"),
            (ECMA_HTML, "ECMA-262"),
            (FIRE_DOM, "DOM"),
        ] {
            let state = extract_html(html, spec);
            assert!(!state.sources.is_empty(), "{spec}: no sources");
        }
    }
}
