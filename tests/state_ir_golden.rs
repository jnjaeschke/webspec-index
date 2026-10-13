//! Golden set for the full statement IR (spec §14.2) over verbatim excerpts.
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use webspec_index::state::bind::{bind, BoundVia};
use webspec_index::state::ir::{BlockRole, CmpOp, Predicate, RunContext, StatementKind, Test};
use webspec_index::state::model::{Passing, StateSpec, TemplatePiece};
use webspec_index::state::query_algorithm::arg_value_text;
use webspec_index::state::render::expr_text;
#[cfg(test)]
use webspec_index::state::testing::index_offline;
use webspec_index::state::testing::{calls_at, statements_at, ty};

#[derive(Deserialize)]
struct Expectations {
    #[serde(default)]
    complete_calls: Vec<String>,
    signatures: Vec<SigE>,
    calls: Vec<CallE>,
    #[serde(default)]
    not_callees: Vec<NotCallee>,
    #[serde(default)]
    statements: Vec<StmtE>,
    #[serde(default)]
    asserts: Vec<AssertE>,
    #[serde(default)]
    opaque_asserts: Vec<Site>,
}
#[derive(Deserialize)]
struct SigE {
    algorithm: String,
    form: String,
    #[serde(default)]
    template: Option<Vec<String>>,
    params: Vec<ParamE>,
    returns: Option<String>,
    #[serde(default)]
    this: Option<String>,
}
#[derive(Deserialize)]
struct ParamE {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    passing: String,
    optional: bool,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    anchor: Option<String>,
}
#[derive(Deserialize)]
struct CallE {
    caller: String,
    step: String,
    callee: String,
    form: String,
    #[serde(default)]
    confidence: Option<String>,
    #[serde(default)]
    args: BTreeMap<String, String>,
    #[serde(default)]
    defaulted: Option<usize>,
}
#[derive(Deserialize)]
struct NotCallee {
    algorithm: String,
    link_text: String,
}
#[derive(Deserialize)]
struct StmtE {
    algorithm: String,
    step: String,
    kinds: Vec<String>,
    #[serde(default)]
    parents: Vec<Option<String>>,
}
#[derive(Deserialize)]
struct AssertE {
    algorithm: String,
    step: String,
    predicate: String,
}
#[derive(Deserialize)]
struct Site {
    algorithm: String,
    step: String,
}

fn split(selector: &str) -> (&str, &str) {
    selector.split_once('#').expect("SPEC#anchor")
}

/// The snake_case tag of a unit variant, or the key of a struct variant.
fn tag<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value).unwrap() {
        serde_json::Value::String(s) => s,
        serde_json::Value::Object(m) => m.keys().next().cloned().unwrap_or_default(),
        other => other.to_string(),
    }
}

fn template_notation(pieces: &[TemplatePiece]) -> Vec<String> {
    pieces
        .iter()
        .map(|p| match p {
            TemplatePiece::Head(w) => format!("head:{w}"),
            TemplatePiece::Callee => "callee".into(),
            TemplatePiece::Literal(l) => format!("lit:{l}"),
            TemplatePiece::Slot(i) => format!("slot:{i}"),
            TemplatePiece::ListSep => "listsep".into(),
            TemplatePiece::NamedGroup(w) => format!("named:{w}"),
        })
        .collect()
}

fn passing_notation(p: &Passing) -> String {
    match p {
        Passing::Positional { index } => format!("positional:{index}"),
        Passing::Named => "named".into(),
        Passing::Receiver => "receiver".into(),
    }
}

fn list(items: &[webspec_index::state::ir::Expr]) -> String {
    items.iter().map(expr_text).collect::<Vec<_>>().join(", ")
}

fn predicate_notation(p: &Predicate) -> String {
    let not = |negated: bool, name: &str| {
        if negated {
            format!("not_{name}")
        } else {
            name.to_string()
        }
    };
    match p {
        Predicate::Is {
            operand,
            test,
            negated,
        } => {
            let x = expr_text(operand);
            match (test, negated) {
                (Test::Null, false) => format!("is_null({x})"),
                (Test::Null, true) => format!("is_not_null({x})"),
                (Test::Set, true) => format!("is_unset({x})"),
                (Test::Type(t), n) => format!("{}({x}, {})", not(*n, "is_type"), ty(t)),
                (t, n) => format!("{}({x})", not(*n, &format!("is_{}", tag(t)))),
            }
        }
        Predicate::Compare {
            lhs,
            op,
            rhs,
            negated,
        } => {
            let name = match op {
                CmpOp::Eq => "eq",
                CmpOp::Lt => "lt",
                CmpOp::Le => "le",
                CmpOp::Gt => "gt",
                CmpOp::Ge => "ge",
            };
            format!(
                "{}({}, {})",
                not(*negated, name),
                expr_text(lhs),
                expr_text(rhs)
            )
        }
        Predicate::OneOf {
            operand,
            values,
            negated,
        } => format!(
            "{}({}, [{}])",
            not(*negated, "one_of"),
            expr_text(operand),
            list(values)
        ),
        Predicate::Contains {
            container,
            item,
            negated,
        } => format!(
            "{}({}, {})",
            not(*negated, "contains"),
            expr_text(container),
            expr_text(item)
        ),
        Predicate::Exists { operand, negated } => {
            format!("{}({})", not(*negated, "exists"), expr_text(operand))
        }
        Predicate::HasAttribute {
            element,
            name,
            negated,
        } => format!(
            "{}({}, {name})",
            not(*negated, "has_attribute"),
            expr_text(element)
        ),
        Predicate::Holds {
            subjects, target, ..
        } => format!(
            "holds({}, {})",
            list(subjects),
            target
                .as_ref()
                .map_or(String::new(), |t| format!("{}#{}", t.spec, t.anchor))
        ),
        Predicate::RunningOn {
            context: RunContext::InParallel,
        } => "running_in_parallel".into(),
        Predicate::RunningOn {
            context: RunContext::Queue(q),
        } => format!("running_on_queue({})", expr_text(q)),
        Predicate::RunningOn {
            context: RunContext::EventLoopTask(x),
        } => format!("event_loop_task({})", expr_text(x)),
        Predicate::And(v) => format!(
            "and({})",
            v.iter()
                .map(predicate_notation)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Predicate::Or(v) => format!(
            "or({})",
            v.iter()
                .map(predicate_notation)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Predicate::Implies(a, b) => format!(
            "implies({}, {})",
            predicate_notation(a),
            predicate_notation(b)
        ),
        Predicate::Opaque { text } => format!("opaque({text})"),
    }
}

#[derive(Default)]
pub struct Check {
    pub failures: Vec<String>,
    pub gaps: Vec<String>,
}

pub fn check(conn: &rusqlite::Connection) -> Check {
    let e: Expectations =
        serde_json::from_str(include_str!("fixtures/state/ir/expectations.json")).unwrap();
    let mut specs: BTreeMap<String, StateSpec> = BTreeMap::new();
    for spec in ["HTML", "DOM"] {
        if let Some(id) = webspec_index::db::queries::get_snapshot(conn, spec).unwrap() {
            specs.insert(
                spec.to_string(),
                webspec_index::db::state::load_state_model(conn, id)
                    .unwrap()
                    .unwrap(),
            );
        }
    }
    let sig_of = |selector: &str| {
        let (spec, anchor) = split(selector);
        specs
            .get(spec)
            .and_then(|s| s.signatures.iter().find(|g| g.algorithm.anchor == anchor))
    };
    let mut out = Check::default();
    for s in &e.signatures {
        let Some(g) = sig_of(&s.algorithm) else {
            out.failures
                .push(format!("signature {}: missing", s.algorithm));
            continue;
        };
        if g.form.as_str() != s.form {
            out.failures.push(format!(
                "signature {}: expected form {}, found {}",
                s.algorithm,
                s.form,
                g.form.as_str()
            ));
        }
        if let Some(want) = &s.template {
            let got = g.template.as_ref().map(|t| template_notation(&t.pieces));
            if got.as_ref() != Some(want) {
                out.failures.push(format!(
                    "signature {}: expected template {want:?}, found {got:?}",
                    s.algorithm
                ));
            }
        }
        if g.params.len() != s.params.len() {
            out.failures.push(format!(
                "signature {}: expected {} params, found {}",
                s.algorithm,
                s.params.len(),
                g.params.len()
            ));
        }
        for (want, got) in s.params.iter().zip(&g.params) {
            let got_row = (
                got.name.clone(),
                ty(&got.ty),
                passing_notation(&got.passing),
                got.optional,
                got.default.as_ref().map(expr_text),
                got.anchor
                    .as_ref()
                    .map(|a| format!("{}#{}", a.spec, a.anchor)),
            );
            let want_row = (
                want.name.clone(),
                want.ty.clone(),
                want.passing.clone(),
                want.optional,
                want.default.clone(),
                want.anchor.clone(),
            );
            // A matching local IDL anchor is the annotation's spelling for the same IDL type.
            let type_matches = match (&got_row.1, &want_row.1) {
                (got, want) if got == want => true,
                (got, want) => {
                    let (spec, _) = split(&s.algorithm);
                    got.strip_prefix("idl:")
                        .is_some_and(|idl| want == &format!("{spec}#{}", idl.to_ascii_lowercase()))
                }
            };
            let rows_match = got_row.0 == want_row.0
                && type_matches
                && got_row.2 == want_row.2
                && got_row.3 == want_row.3
                // The spec annotation omits the article from a default empty string.
                && (got_row.4 == want_row.4
                    || (got_row.4.as_deref() == Some("the empty string")
                        && want_row.4.as_deref() == Some("\"\"")))
                && got_row.5 == want_row.5;
            if !rows_match {
                out.failures.push(format!(
                    "signature {} param {}: expected {want_row:?}, found {got_row:?}",
                    s.algorithm, want.name
                ));
            }
        }
        let returns = g.returns.as_ref().map(|r| ty(&r.ty));
        if returns != s.returns {
            out.failures.push(format!(
                "signature {}: expected returns {:?}, found {returns:?}",
                s.algorithm, s.returns
            ));
        }
        let this = g.this.as_ref().map(ty);
        if s.this.is_some() && this != s.this {
            out.failures.push(format!(
                "signature {}: expected this {:?}, found {this:?}",
                s.algorithm, s.this
            ));
        }
    }
    let rank = |c: &str| match c {
        "exact" => 2,
        "partial" => 1,
        _ => 0,
    };
    let mut named: BTreeSet<String> = BTreeSet::new();
    for c in &e.calls {
        let site = format!("{}:{} → {}", c.caller, c.step, c.callee);
        let (spec, anchor) = split(&c.caller);
        let Some(state) = specs.get(spec) else {
            out.failures.push(format!("call {site}: spec not indexed"));
            continue;
        };
        let calls = calls_at(state, anchor, &c.step);
        let found = calls.iter().find(|k| {
            !named.contains(&k.id)
                && k.callee
                    .target
                    .as_ref()
                    .map(|t| format!("{}#{}", t.spec, t.anchor))
                    .as_deref()
                    == Some(c.callee.as_str())
        });
        let Some(call) = found else {
            out.failures.push(format!(
                "call {site}: missing (have {:?})",
                calls
                    .iter()
                    .map(|k| &k.callee.visible_text)
                    .collect::<Vec<_>>()
            ));
            continue;
        };
        named.insert(call.id.clone());
        if tag(&call.form) != c.form {
            out.failures.push(format!(
                "call {site}: expected form {}, found {}",
                c.form,
                tag(&call.form)
            ));
        }
        let Some(want_confidence) = &c.confidence else {
            continue;
        };
        let Some(sig) = sig_of(&c.callee) else {
            out.failures.push(format!(
                "call {site}: bound entry, but the callee has no signature in the excerpts"
            ));
            continue;
        };
        let source = state
            .sources
            .iter()
            .find(|s| s.id == call.source_id)
            .unwrap();
        let binding = bind(call, source, sig);
        let mut wrong = false;
        for name in c
            .args
            .keys()
            .filter(|n| !sig.params.iter().any(|p| &&p.name == n))
        {
            wrong = true;
            out.failures
                .push(format!("call {site}: no parameter {name}"));
        }
        for (param, arg) in sig.params.iter().zip(&binding.args) {
            let got = arg_value_text(&source.text, arg);
            match (c.args.get(&param.name), got) {
                // Annotation notation drops a leading article from argument text.
                (Some(want), Some(got)) if got.strip_prefix("the ").unwrap_or(&got) != want => {
                    wrong = true;
                    out.failures.push(format!(
                        "call {site} arg {}: expected {want}, found {got}",
                        param.name
                    ));
                }
                (None, Some(got)) => {
                    wrong = true;
                    out.failures.push(format!(
                        "call {site} arg {}: not annotated, found {got}",
                        param.name
                    ));
                }
                (Some(want), None) => out.gaps.push(format!(
                    "call {site} arg {}: {want} left unbound {:?}",
                    param.name, binding.issues
                )),
                _ => {}
            }
        }
        let got_confidence = tag(&binding.confidence);
        let lower = rank(&got_confidence) < rank(want_confidence);
        if rank(&got_confidence) > rank(want_confidence) {
            out.failures.push(format!(
                "call {site}: expected {want_confidence}, found {got_confidence}"
            ));
        } else if lower && !wrong {
            out.gaps.push(format!(
                "call {site}: expected {want_confidence}, found {got_confidence} {:?}",
                binding.issues
            ));
        }
        if let Some(want) = c.defaulted {
            let got = binding
                .args
                .iter()
                .filter(|a| a.via == BoundVia::Default)
                .count();
            if got != want {
                let line = format!("call {site}: expected {want} defaulted, found {got}");
                if lower {
                    out.gaps.push(line)
                } else {
                    out.failures.push(line)
                }
            }
        }
    }
    for algorithm in &e.complete_calls {
        let (spec, anchor) = split(algorithm);
        let Some(state) = specs.get(spec) else {
            continue;
        };
        let own: BTreeSet<&str> = state
            .sources
            .iter()
            .filter(|s| s.subject.anchor == anchor)
            .map(|s| s.id.as_str())
            .collect();
        for call in state
            .calls
            .iter()
            .filter(|k| own.contains(k.source_id.as_str()) && !named.contains(&k.id))
        {
            out.failures.push(format!(
                "call {algorithm}: extra callee {} ({})",
                call.callee.visible_text,
                call.callee
                    .target
                    .as_ref()
                    .map_or(String::new(), |t| format!("{}#{}", t.spec, t.anchor))
            ));
        }
    }
    for n in &e.not_callees {
        let (spec, anchor) = split(&n.algorithm);
        let Some(state) = specs.get(spec) else {
            continue;
        };
        for source in state.sources.iter().filter(|s| s.subject.anchor == anchor) {
            for link in source
                .links
                .iter()
                .filter(|l| l.visible_text == n.link_text)
            {
                if state
                    .calls
                    .iter()
                    .any(|c| c.source_id == source.id && c.callee.link_id == link.id)
                {
                    out.failures.push(format!(
                        "not_callee {}: {} is a callee",
                        n.algorithm, n.link_text
                    ));
                }
            }
        }
    }
    for s in &e.statements {
        let (spec, anchor) = split(&s.algorithm);
        let Some(state) = specs.get(spec) else {
            continue;
        };
        let st = statements_at(state, anchor, &s.step);
        let kinds: Vec<String> = if st
            .first()
            .is_none_or(|x| matches!(x.kind, StatementKind::Opaque { .. }))
            && s.kinds == ["unrecognized"]
        {
            vec!["unrecognized".into()]
        } else {
            st.iter().map(|x| tag(&x.kind)).collect()
        };
        if kinds != s.kinds {
            out.failures.push(format!(
                "statements {}:{}: expected {:?}, found {kinds:?}",
                s.algorithm, s.step, s.kinds
            ));
        }
        for (i, want) in s.parents.iter().enumerate() {
            let got = st.get(i).and_then(|x| x.parent.as_ref()).map(|p| {
                let kind = st
                    .iter()
                    .find(|y| y.id == p.statement_id)
                    .map(|y| tag(&y.kind))
                    .unwrap_or_else(|| "outside".into());
                format!(
                    "{kind}:{}",
                    match p.role {
                        BlockRole::Then => "then",
                        BlockRole::Otherwise => "otherwise",
                        BlockRole::Body => "body",
                    }
                )
            });
            if &got != want {
                out.failures.push(format!(
                    "statements {}:{} #{i}: expected parent {want:?}, found {got:?}",
                    s.algorithm, s.step
                ));
            }
        }
    }
    let assert_of = |site_alg: &str, step: &str| -> Option<Predicate> {
        let (spec, anchor) = split(site_alg);
        statements_at(specs.get(spec)?, anchor, step)
            .into_iter()
            .find_map(|x| match &x.kind {
                StatementKind::Assert { predicate } => Some(predicate.clone()),
                _ => None,
            })
    };
    for a in &e.asserts {
        match assert_of(&a.algorithm, &a.step) {
            Some(p) if predicate_notation(&p) == a.predicate => {}
            Some(p) => out.failures.push(format!(
                "assert {}:{}: expected {}, found {}",
                a.algorithm,
                a.step,
                a.predicate,
                predicate_notation(&p)
            )),
            None => out
                .failures
                .push(format!("assert {}:{}: missing", a.algorithm, a.step)),
        }
    }
    for o in &e.opaque_asserts {
        if !matches!(
            assert_of(&o.algorithm, &o.step),
            Some(Predicate::Opaque { .. })
        ) {
            out.failures.push(format!(
                "opaque assert {}:{}: parsed or missing",
                o.algorithm, o.step
            ));
        }
    }
    out
}

#[test]
fn golden_signatures_calls_and_asserts_on_excerpts() {
    let conn = webspec_index::db::open_in_memory().unwrap();
    index_offline(
        &conn,
        "HTML",
        "https://html.spec.whatwg.org/",
        include_str!("fixtures/state/ir/html.html"),
    )
    .unwrap();
    index_offline(
        &conn,
        "DOM",
        "https://dom.spec.whatwg.org/",
        include_str!("fixtures/state/ir/dom.html"),
    )
    .unwrap();
    let result = check(&conn);
    for gap in &result.gaps {
        eprintln!("GAP {gap}");
    }
    eprintln!("{} gaps", result.gaps.len());
    let mut got = result.failures;
    got.sort();
    got.dedup();
    // The ratchet contains only known GOLDEN failures; GAP lines remain informational.
    let mut known: Vec<String> = include_str!("fixtures/state/ir/known_drift.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(str::to_string)
        .collect();
    known.sort();
    known.dedup();
    let new: Vec<_> = got.iter().filter(|line| !known.contains(line)).collect();
    let fixed: Vec<_> = known.iter().filter(|line| !got.contains(line)).collect();
    assert!(
        new.is_empty() && fixed.is_empty(),
        "new drift:\n{}\nfixed drift (remove from known_drift.txt):\n{}",
        new.iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        fixed
            .iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
