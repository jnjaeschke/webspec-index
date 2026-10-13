//! The algorithm view of `state SPEC#anchor` (§16.6): an algorithm's
//! signature, statement counts and calls, each call bound to its callee's
//! signature at query time.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::state::{self as db, StateSnapshot, StoredCall};
use crate::parse::steps::AnchorTarget;
use crate::state::bind::{bind, ArgValue, Binding, BindingIssue, BoundVia, Confidence};
use crate::state::ir::CallForm;
use crate::state::model::{Passing, ReviewItem, Signature, StateIssueCode, TemplatePiece};
use crate::state::query::{
    anchor_url, StateError, StateErrorCode, StateQueryOptions, StateStatus, StatusCounts,
};
use crate::state::render;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateAlgorithmResult {
    pub algorithm: String,
    pub name: String,
    pub url: String,
    pub signature: Option<Signature>,
    pub template: Option<String>,
    pub statements: BTreeMap<String, u32>,
    pub opaque_count: u32,
    pub opaque: Option<Vec<ReviewItem>>,
    pub calls: Option<Vec<CallView>>,
    pub callers: Option<CallerList>,
    pub unbound_calls: u32,
    pub status: StateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallView {
    pub step_path: Option<String>,
    pub callee: String,
    pub callee_name: String,
    pub form: CallForm,
    pub confidence: Confidence,
    pub args: Vec<ArgView>,
    pub defaulted: u32,
    pub defaulted_named: bool,
    pub issues: Vec<BindingIssue>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgView {
    pub param: String,
    pub value: String,
    pub via: BoundVia,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerList {
    pub total: u32,
    pub groups: Vec<CallerGroup>,
    pub more: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerGroup {
    pub caller: String,
    pub calls: Vec<CallView>,
}

/// The algorithm view of `spec#anchor`, or `None` when the anchor has neither
/// an algorithm summary nor a signature.
pub(crate) fn algorithm_view(
    conn: &Connection,
    snapshots: &[StateSnapshot],
    spec: &str,
    anchor: &str,
    options: &StateQueryOptions,
) -> Result<Option<StateAlgorithmResult>, StateError> {
    let spec_ids = ids_of(snapshots, spec);
    let snapshot = snapshots.iter().find(|s| s.spec == spec);
    if let Some(snapshot) = snapshot {
        let version = db::state_version_of(conn, snapshot.id)?;
        if version.as_deref().and_then(|v| v.split('+').next()) != Some(crate::state::STATE_VERSION)
        {
            if db::section_type(conn, &spec_ids, anchor)?.as_deref() == Some("algorithm") {
                return Err(StateError::new(
                    StateErrorCode::SpecNotIndexed,
                    format!("the state model of {spec} was built by an older version; run `webspec-index update`"),
                ));
            }
            return Ok(None);
        }
    }
    let summary = db::algorithm_summary(conn, &spec_ids, spec, anchor)?;
    let signature = db::signature_for(conn, &spec_ids, spec, anchor)?;
    if summary.is_none() && signature.is_none() {
        return Ok(None);
    }
    let name = db::section_title(conn, &spec_ids, anchor)?
        .map_or_else(|| anchor.to_string(), |title| one_line(&title));
    let base_url = snapshots
        .iter()
        .find(|s| s.spec == spec)
        .map_or("", |s| s.base_url.as_str());
    let template = signature
        .as_ref()
        .filter(|sig| sig.template.is_some())
        .map(|sig| render_template(sig, &name));
    let (statements, opaque) = summary.map_or_else(Default::default, |s| (s.statements, s.opaque));

    let mut stored = db::calls_by_subject(conn, &spec_ids, spec, anchor)?;
    stored.sort_by_cached_key(|c| {
        (
            step_key(c.step_path.as_deref()),
            c.offset + c.call.span.start,
        )
    });
    let targets: Vec<AnchorTarget> = stored
        .iter()
        .filter_map(|c| c.call.callee.target.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let all_ids: Vec<i64> = snapshots.iter().map(|s| s.id).collect();
    let signatures = db::signatures_for_targets(conn, &all_ids, &targets)?;
    let mut titles: HashMap<AnchorTarget, Option<String>> = HashMap::new();
    let mut unbound_calls = 0;
    let mut issues = BTreeSet::new();
    let mut views = Vec::new();
    for call in &stored {
        let target = call.call.callee.target.as_ref();
        let callee_signature = target.and_then(|t| signatures.get(t));
        let binding = match callee_signature {
            Some(sig) => bind(&call.call, &call.source, sig),
            None => {
                let mut binding = unsigned_binding(call);
                if target.is_some_and(|t| !snapshots.iter().any(|s| s.spec == t.spec)) {
                    binding.issues.push(BindingIssue::MissingSpec);
                    issues.insert(StateIssueCode::MissingSpec);
                }
                binding
            }
        };
        if binding.confidence == Confidence::Unbound {
            unbound_calls += 1;
        }
        if !options.calls {
            continue;
        }
        let title = match target {
            Some(t) if snapshots.iter().any(|s| s.spec == t.spec) => {
                if !titles.contains_key(t) {
                    let title = db::section_title(conn, &ids_of(snapshots, &t.spec), &t.anchor)?;
                    titles.insert(t.clone(), title);
                }
                titles[t].clone()
            }
            _ => None,
        };
        let callee_name = one_line(title.as_deref().unwrap_or(&call.call.callee.visible_text));
        views.push(call_view(call, &binding, callee_signature, &callee_name));
    }
    let callers = if options.callers {
        Some(caller_list(
            conn,
            &all_ids,
            spec,
            anchor,
            signature.as_ref(),
            &name,
            options.limit.unwrap_or(CALLER_LIMIT),
        )?)
    } else {
        None
    };
    let opaque_count = opaque.len() as u32;
    Ok(Some(StateAlgorithmResult {
        algorithm: format!("{spec}#{anchor}"),
        name,
        url: anchor_url(base_url, anchor),
        signature,
        template,
        statements,
        opaque_count,
        opaque: options.opaque.then_some(opaque),
        calls: options.calls.then_some(views),
        callers,
        unbound_calls,
        status: StateStatus::new(
            opaque_count == 0 && unbound_calls == 0,
            issues,
            StatusCounts::default(),
        ),
    }))
}

/// Caller algorithms listed when `--limit` is not given.
const CALLER_LIMIT: u32 = 20;

/// The calls into `spec#anchor` from every indexed spec, grouped by calling
/// algorithm and bound against `signature`; groups past `limit` are counted.
fn caller_list(
    conn: &Connection,
    snapshot_ids: &[i64],
    spec: &str,
    anchor: &str,
    signature: Option<&Signature>,
    name: &str,
    limit: u32,
) -> Result<CallerList, StateError> {
    let page = db::caller_groups(conn, snapshot_ids, spec, anchor, limit)?;
    let shown: Vec<(i64, String)> = page
        .groups
        .iter()
        .map(|(snapshot, _, subject)| (*snapshot, subject.clone()))
        .collect();
    let stored = db::calls_of_callers(conn, spec, anchor, &shown)?;
    let shown_groups = page.groups.len() as u32;
    let mut groups = Vec::with_capacity(page.groups.len());
    for (snapshot_id, caller_spec, subject) in page.groups {
        let caller = format!("{caller_spec}#{subject}");
        let mut calls: Vec<_> = stored
            .iter()
            .filter(|call| call.snapshot_id == snapshot_id && call.subject == subject)
            .collect();
        calls.sort_by_cached_key(|c| {
            (
                step_key(c.step_path.as_deref()),
                c.offset + c.call.span.start,
            )
        });
        let calls = calls
            .into_iter()
            .map(|call| {
                let binding = match signature {
                    Some(sig) => bind(&call.call, &call.source, sig),
                    None => unsigned_binding(call),
                };
                call_view(call, &binding, signature, name)
            })
            .collect();
        groups.push(CallerGroup { caller, calls });
    }
    Ok(CallerList {
        total: page.total_calls,
        groups,
        more: page.total_groups.saturating_sub(shown_groups),
    })
}

/// `text` with every whitespace run, line breaks included, as one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ids_of(snapshots: &[StateSnapshot], spec: &str) -> Vec<i64> {
    snapshots
        .iter()
        .filter(|s| s.spec == spec)
        .map(|s| s.id)
        .collect()
}

/// Step paths in document order: numeric components compare as numbers.
fn step_key(step_path: Option<&str>) -> Vec<(u64, String)> {
    step_path.map_or_else(Vec::new, |path| {
        path.split('.')
            .map(|part| match part.parse::<u64>() {
                Ok(n) => (n, String::new()),
                Err(_) => (u64::MAX, part.to_string()),
            })
            .collect()
    })
}

/// The binding of a call whose callee has no signature: nothing aligns.
fn unsigned_binding(stored: &StoredCall) -> Binding {
    let callee = &stored.call.callee;
    Binding {
        call: stored.call.id.clone(),
        callee: callee.target.clone().unwrap_or_else(|| AnchorTarget {
            spec: String::new(),
            anchor: callee.visible_text.clone(),
        }),
        args: Vec::new(),
        initializers: Vec::new(),
        confidence: Confidence::Unbound,
        issues: vec![BindingIssue::NoSignature],
    }
}

/// The signature's call template as prose: `navigate {navigable} to {url}`.
pub fn render_template(signature: &Signature, callee_name: &str) -> String {
    let Some(template) = &signature.template else {
        return String::new();
    };
    let mut out = String::new();
    let mut after_open = true;
    for piece in &template.pieces {
        let word = match piece {
            TemplatePiece::Head(words) | TemplatePiece::Literal(words) => words.clone(),
            TemplatePiece::Callee => callee_name.to_string(),
            TemplatePiece::Slot(index) => {
                let name = signature
                    .params
                    .iter()
                    .find(|p| p.passing == Passing::Positional { index: *index })
                    .map_or_else(|| index.to_string(), |p| p.name.clone());
                format!("{{{name}}}")
            }
            TemplatePiece::ListSep => ",".to_string(),
            TemplatePiece::NamedGroup(_) => ", with named arguments".to_string(),
        };
        let glued = [",", "'s", ")"].iter().any(|p| word.starts_with(p));
        if !out.is_empty() && !after_open && !glued {
            out.push(' ');
        }
        after_open = word.ends_with('(');
        out.push_str(&word);
    }
    out
}

/// One call bound against its callee (`signature` is the callee's).
pub(crate) fn call_view(
    stored: &StoredCall,
    binding: &Binding,
    signature: Option<&Signature>,
    callee_name: &str,
) -> CallView {
    let callee = &stored.call.callee;
    let param = |index: u32| signature.and_then(|sig| sig.params.get(index as usize));
    let mut args = Vec::new();
    let mut defaulted = 0;
    let mut defaulted_named = true;
    for arg in &binding.args {
        let value = match &arg.value {
            ArgValue::Default(_) => {
                defaulted += 1;
                defaulted_named &= param(arg.param).is_some_and(|p| p.passing == Passing::Named);
                continue;
            }
            ArgValue::Unbound => continue,
            // A named argument's span covers `name set to value`, not the value.
            ArgValue::Expr(expr) if matches!(arg.via, BoundVia::Name(_)) => render::expr_text(expr),
            ArgValue::Expr(expr) => arg
                .span
                .and_then(|span| stored.source.text.get(span.start..span.end))
                .map_or_else(|| render::expr_text(expr), str::to_string),
            // A body argument names a steps source, not text.
            ArgValue::Body(_) => arg
                .span
                .and_then(|span| stored.source.text.get(span.start..span.end))
                .map_or_else(|| "(steps)".to_string(), str::to_string),
        };
        args.push(ArgView {
            param: param(arg.param).map_or_else(|| arg.param.to_string(), |p| p.name.clone()),
            value: one_line(&value),
            via: arg.via.clone(),
        });
    }
    CallView {
        step_path: stored.step_path.clone(),
        callee: callee.target.as_ref().map_or_else(
            || callee.visible_text.clone(),
            |t| format!("{}#{}", t.spec, t.anchor),
        ),
        callee_name: callee_name.to_string(),
        form: stored.call.form,
        confidence: binding.confidence,
        args,
        defaulted,
        defaulted_named,
        issues: binding.issues.clone(),
        text: stored.source.text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::bind::{BindingIssue, Confidence};
    use crate::state::query::{query, StateQueryOptions, StateResponse};
    use crate::state::testing::db_with;
    use crate::state::testing::NAV_HTML;

    fn view(
        conn: &rusqlite::Connection,
        selector: &str,
        calls: bool,
        callers: bool,
        opaque: bool,
        limit: Option<u32>,
    ) -> StateAlgorithmResult {
        let options = StateQueryOptions {
            calls,
            callers,
            opaque,
            limit,
            ..StateQueryOptions::default()
        };
        match query(conn, selector, &options)
            .unwrap_or_else(|e| panic!("{selector}: {}", e.message))
        {
            StateResponse::Algorithm(v) => v,
            other => panic!("{selector} is not an algorithm view: {other:?}"),
        }
    }

    const EXTRA_CALLERS: &str = r##"<div data-algorithm=""><p>To <dfn id="reload">reload</dfn> a <a href="#navigable">navigable</a> <var>n</var>:</p><ol><li><p><a href="#navigate">Navigate</a> <var>n</var> to <var>n</var>'s <a href="#nav-url">URL</a>.</p></li><li><p>Add <var>n</var> to the list of reloaded things.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="go">go</dfn> given <var>n</var>:</p><ol><li><p><a href="#navigate">Navigate</a> <var>n</var> to <var>u</var>.</p></li></ol></div>"##;

    #[test]
    fn navigate_view_template_and_statements() {
        let conn = db_with(&[("HTML", NAV_HTML)]);
        let v = view(&conn, "HTML#navigate", false, false, false, None);
        assert_eq!(v.algorithm, "HTML#navigate");
        assert_eq!(
            v.template.as_deref(),
            Some("navigate {navigable} to {url} using {sourceDocument}, with named arguments")
        );
        assert_eq!(v.signature.as_ref().unwrap().params.len(), 6);
        assert_eq!(v.statements.get("in_parallel"), Some(&1));
        assert!(v.calls.is_none() && v.callers.is_none() && v.opaque.is_none());
    }

    #[test]
    fn calls_with_bindings_and_defaulted_count() {
        let conn = db_with(&[("HTML", NAV_HTML)]);
        let v = view(
            &conn,
            "HTML#location-object-navigate",
            true,
            false,
            false,
            None,
        );
        let calls = v.calls.unwrap();
        assert_eq!(calls.len(), 1);
        let c = &calls[0];
        assert_eq!(
            (
                c.step_path.as_deref(),
                c.callee.as_str(),
                c.callee_name.as_str(),
                c.confidence
            ),
            (Some("4"), "HTML#navigate", "navigate", Confidence::Exact)
        );
        assert_eq!(
            c.args
                .iter()
                .map(|a| (a.param.as_str(), a.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("navigable", "*navigable*"),
                ("url", "*url*"),
                ("sourceDocument", "*sourceDocument*"),
                ("exceptionsEnabled", "true"),
                ("historyHandling", "*historyHandling*")
            ]
        );
        assert_eq!((c.defaulted, c.defaulted_named), (1, true));
        assert_eq!(v.unbound_calls, 0);
    }

    #[test]
    fn opaque_statements_and_callee_without_signature() {
        let html = format!("{NAV_HTML}{EXTRA_CALLERS}");
        let conn = db_with(&[("HTML", html.as_str())]);
        let v = view(&conn, "HTML#reload", true, false, true, None);
        assert_eq!(v.opaque_count, 1);
        assert_eq!(v.opaque.unwrap()[0].step_path.as_deref(), Some("2"));
        assert_eq!(v.status.coverage, crate::state::query::Coverage::Partial);
        let other = r##"<div data-algorithm=""><p>To <dfn id="x">x</dfn> given a <var>n</var>:</p><ol><li><p><a href="https://dom.spec.whatwg.org/#concept-node-remove">Remove</a> <var>n</var>.</p></li></ol></div>"##;
        let conn = db_with(&[("HTML", other)]);
        let c = view(&conn, "HTML#x", true, false, false, None)
            .calls
            .unwrap()
            .remove(0);
        assert_eq!(
            (c.confidence, c.issues.first()),
            (Confidence::Unbound, Some(&BindingIssue::NoSignature))
        );
        assert_eq!(
            (c.callee.as_str(), c.callee_name.as_str()),
            ("DOM#concept-node-remove", "Remove")
        );
        let v = view(&conn, "HTML#x", false, false, false, None);
        assert_eq!(
            (v.unbound_calls, v.status.coverage),
            (1, crate::state::query::Coverage::Partial)
        );
    }

    #[test]
    fn callers_are_grouped_bound_and_capped() {
        let html = format!("{NAV_HTML}{EXTRA_CALLERS}");
        let conn = db_with(&[("HTML", html.as_str())]);
        let v = view(&conn, "HTML#navigate", false, true, false, Some(2));
        let callers = v.callers.unwrap();
        assert_eq!(
            (callers.total, callers.groups.len(), callers.more),
            (3, 2, 1)
        );
        assert_eq!(
            callers
                .groups
                .iter()
                .map(|g| g.caller.as_str())
                .collect::<Vec<_>>(),
            ["HTML#go", "HTML#location-object-navigate"]
        );
        let snapshots: Vec<i64> = crate::db::state::state_snapshots(&conn)
            .unwrap()
            .iter()
            .map(|s| s.id)
            .collect();
        let page =
            crate::db::state::caller_groups(&conn, &snapshots, "HTML", "navigate", 2).unwrap();
        assert_eq!(
            (page.total_calls, page.total_groups, page.groups.len()),
            (3, 3, 2)
        );
        let shown: Vec<(i64, String)> = page
            .groups
            .iter()
            .map(|(snapshot, _, subject)| (*snapshot, subject.clone()))
            .collect();
        assert_eq!(
            crate::db::state::calls_of_callers(&conn, "HTML", "navigate", &shown)
                .unwrap()
                .len(),
            2
        );
        let go = &callers.groups[0].calls[0];
        assert_eq!(
            (go.step_path.as_deref(), go.callee.as_str(), go.confidence),
            (Some("1"), "HTML#navigate", Confidence::Exact)
        );
        assert_eq!(
            go.args
                .iter()
                .map(|a| (a.param.as_str(), a.value.as_str()))
                .collect::<Vec<_>>(),
            [("navigable", "*n*"), ("url", "*u*")]
        );
        let all = view(&conn, "HTML#navigate", false, true, false, None)
            .callers
            .unwrap();
        assert_eq!((all.groups.len(), all.more), (3, 0));
        assert!(view(&conn, "HTML#navigate", false, false, false, None)
            .callers
            .is_none());
    }

    #[test]
    fn stale_snapshot_is_spec_not_indexed_for_algorithms_only() {
        use crate::state::query::StateErrorCode;

        let conn = db_with(&[("HTML", NAV_HTML)]);
        conn.execute_batch(
            "DELETE FROM state_algorithms; DELETE FROM state_signatures; DELETE FROM state_calls;",
        )
        .unwrap();
        let current = crate::state::STATE_VERSION;
        for (stored, stale) in [
            (None, true),
            (Some("0+sha256:ab".to_string()), true),
            (Some(format!("{current}0")), true),
            (Some(format!("{current}+sha256:ab")), false),
        ] {
            conn.execute(
                "UPDATE state_coverage SET representation_version = ?1",
                [&stored],
            )
            .unwrap();
            let answer = query(&conn, "HTML#navigate", &StateQueryOptions::default());
            if stale {
                let err = answer.unwrap_err();
                assert_eq!(err.code, StateErrorCode::SpecNotIndexed, "{stored:?}");
                assert!(
                    err.message.contains("webspec-index update"),
                    "{}",
                    err.message
                );
            } else {
                assert!(
                    !matches!(answer, Err(ref e) if e.message.contains("older version")),
                    "{stored:?}: current version with a digest is not stale"
                );
            }
            assert!(
                matches!(
                    query(
                        &conn,
                        "HTML#ongoing-navigation",
                        &StateQueryOptions::default()
                    ),
                    Ok(StateResponse::Field(_))
                ),
                "{stored:?}: sites fallback survives"
            );
        }
    }

    #[test]
    fn call_into_unindexed_spec_is_unbound_not_fatal() {
        let html = r##"<div data-algorithm=""><p>To <dfn id="x">x</dfn> given a <var>n</var> and a <var>p</var>:</p><ol><li><p><a href="https://dom.spec.whatwg.org/#concept-node-insert">Insert</a> <var>n</var> into <var>p</var> before null.</p></li></ol></div>"##;
        let conn = db_with(&[("HTML", html)]);
        let v = view(&conn, "HTML#x", true, false, false, None);
        let c = &v.calls.as_ref().unwrap()[0];
        assert_eq!(c.confidence, Confidence::Unbound);
        assert_eq!(
            c.issues,
            vec![BindingIssue::NoSignature, BindingIssue::MissingSpec]
        );
        assert!(v
            .status
            .issues
            .contains(&crate::state::model::StateIssueCode::MissingSpec));
        let conn = db_with(&[("HTML", html), ("DOM", crate::state::testing::INSERT_DOM)]);
        let c = view(&conn, "HTML#x", true, false, false, None)
            .calls
            .unwrap()
            .remove(0);
        assert_eq!(c.confidence, Confidence::Exact);
        assert_eq!(
            c.args.iter().map(|a| a.value.as_str()).collect::<Vec<_>>(),
            ["*n*", "*p*", "null"]
        );
    }

    #[test]
    fn body_argument_and_wrapped_callee_title() {
        let html = r##"<div data-algorithm=""><p>To <dfn id="q">queue a
    global task</dfn> on a <a href="#task-source">task source</a> <var>source</var>, with a <a href="#global-object">global object</a> <var>global</var> and a series of steps <var>steps</var>:</p><ol><li><p>Return.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="c">c</dfn> given a <a href="#global-object">global object</a> <var>global</var>:</p><ol>
<li><p><a href="#q">Queue a global task</a> on <var>source</var> given <var>global</var> to run the following steps:</p><ol><li><p>Return.</p></li></ol></li>
</ol></div>"##;
        let conn = db_with(&[("HTML", html)]);
        let c = view(&conn, "HTML#c", true, false, false, None)
            .calls
            .unwrap()
            .remove(0);
        assert_eq!(c.callee_name, "queue a global task");
        assert_eq!(
            c.args
                .iter()
                .map(|a| (a.param.as_str(), a.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("source", "*source*"),
                ("global", "*global*"),
                ("steps", "(steps)")
            ]
        );
        assert_eq!(
            view(&conn, "HTML#q", false, false, false, None).name,
            "queue a global task"
        );
    }

    #[test]
    fn template_spacing_around_punctuation() {
        let conn = db_with(&[("HTML", NAV_HTML)]);
        let mut sig = view(&conn, "HTML#navigate", false, false, false, None)
            .signature
            .unwrap();
        sig.template = Some(crate::state::model::Template {
            pieces: vec![
                TemplatePiece::Head("the".into()),
                TemplatePiece::Callee,
                TemplatePiece::Literal("of".into()),
                TemplatePiece::Slot(0),
                TemplatePiece::Literal("'s".into()),
                TemplatePiece::ListSep,
                TemplatePiece::Literal("(".into()),
                TemplatePiece::Slot(1),
                TemplatePiece::Literal(")".into()),
            ],
        });
        assert_eq!(
            render_template(&sig, "result"),
            "the result of {navigable}'s, ({url})"
        );
    }

    #[test]
    fn step_paths_order_numerically() {
        let mut paths = vec![Some("10"), Some("2.1"), None, Some("2"), Some("9.3")];
        paths.sort_by_key(|p| step_key(*p));
        assert_eq!(
            paths,
            [None, Some("2"), Some("2.1"), Some("9.3"), Some("10")]
        );
    }

    /// Guard for the resolution order: passes before and after this task.
    #[test]
    fn non_algorithm_anchors_still_reach_the_sites_fallback() {
        let conn = db_with(&[("HTML", NAV_HTML)]);
        assert!(matches!(
            query(
                &conn,
                "HTML#ongoing-navigation",
                &StateQueryOptions::default()
            )
            .unwrap(),
            StateResponse::Field(_)
        ));
    }
}
