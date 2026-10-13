//! Whole-corpus golden check and coverage print (spec §12.3–§12.4, statement IR §6.4–§6.6).
//!
//! Usage: cargo run --release --example state_golden -- [--review] HTML.html DOM.html
//! Indexes both snapshots twice: with the bundled catalog (golden set, bundled floors) and
//! with an empty catalog (grammar-only floors). The bundled run also binds every stored call
//! (binding rates, §14.5 floor, §13 time budget). `--review` prints the unclassified review
//! items, unparsed assertions, undeclared variables and non-exact HTML bindings of the
//! bundled run. Exits 1 on any golden mismatch, coverage floor violation or blown budget.
#[path = "../tests/state_golden.rs"]
#[allow(dead_code)]
mod golden;
#[path = "../tests/state_ir_golden.rs"]
#[allow(dead_code)]
mod ir_golden;

use std::collections::{BTreeSet, HashSet};
use webspec_index::db::state::StoredCall;
use webspec_index::state::bind::{bind, Binding, Confidence};
use webspec_index::state::catalog::StateCatalog;
use webspec_index::state::ir::{Expr, Origin, SourceContext, StatementKind};
use webspec_index::state::model::{
    AnchorTarget, Param, ReviewItem, Signature, SignatureForm, StateSpec, TemplatePiece, TypeBasis,
    TypeExpr,
};

struct Floors {
    owner: f64,
    set: f64,
    unclassified: f64,
    /// §14.5: `To`-form parameters typed, including by name and by `data-dfn-for`.
    to_typed: f64,
}

/// §14.5 floor on typed `To` parameters; the type pass does not read the catalog.
fn to_typed_floor(spec: &str) -> f64 {
    if spec == "HTML" {
        88.0
    } else {
        78.0
    }
}

/// §12.4 floors with the bundled catalog: owner resolution is the value measured after the
/// bundled YAML (HTML fields whose owner the spec text does not state stay unresolved), and
/// the verb rules lower the unclassified ceiling.
fn bundled_floors(spec: &str) -> Floors {
    if spec == "HTML" {
        Floors {
            owner: 93.0,
            set: 91.0,
            unclassified: 2.2,
            to_typed: to_typed_floor(spec),
        }
    } else {
        Floors {
            owner: 100.0,
            set: 95.0,
            unclassified: 1.9,
            to_typed: to_typed_floor(spec),
        }
    }
}

/// §12.4 floors with the grammar alone.
fn grammar_floors(spec: &str) -> Floors {
    if spec == "HTML" {
        Floors {
            owner: 77.0,
            set: 91.0,
            unclassified: 2.7,
            to_typed: to_typed_floor(spec),
        }
    } else {
        Floors {
            owner: 94.0,
            set: 95.0,
            unclassified: 2.0,
            to_typed: to_typed_floor(spec),
        }
    }
}

/// Whether a `To`-form parameter counts as typed for §14.5: a prose `Opaque` type does not.
fn is_typed(param: &Param) -> bool {
    match param.type_basis {
        TypeBasis::Explicit => !matches!(param.ty, TypeExpr::Opaque { .. }),
        TypeBasis::NameResolved | TypeBasis::DfnFor => true,
        TypeBasis::Unknown => false,
    }
}

/// Check the statement IR coverage floors from the SP3 plan against one snapshot.
fn report_ir_floors(spec: &str, state: &StateSpec) -> bool {
    let c = &state.coverage;
    let pct = |part: u32, total: u32| 100.0 * part as f64 / total.max(1) as f64;
    let to_params: u32 = c.to_params.values().sum();
    let typed: u32 = ["explicit", "name_resolved", "dfn_for"]
        .iter()
        .filter_map(|basis| c.to_params.get(*basis))
        .sum();
    let (template_floor, typed_floor, idl_floor, head_floor, if_floor, assert_floor, foreach_floor) =
        if spec == "HTML" {
            (73.0, 88.0, 85.0, 93.0, 47.0, 67.0, 81.0)
        } else {
            (91.0, 78.0, 97.0, 98.0, 56.0, 57.0, 63.0)
        };
    let metrics = [
        (
            "template signatures",
            c.template_signatures,
            c.algorithms,
            template_floor,
        ),
        ("To parameters typed", typed, to_params, typed_floor),
        ("IDL from IDL", c.idl_from_idl, c.idl_signatures, idl_floor),
        (
            "step heads recognized",
            c.steps_recognized,
            c.steps,
            head_floor,
        ),
        ("If conditions parsed", c.if_parsed, c.if_total, if_floor),
        (
            "Assert predicates parsed",
            c.assert_parsed,
            c.assert_total,
            assert_floor,
        ),
        (
            "For each loop variables bound",
            c.foreach_bound,
            c.foreach_total,
            foreach_floor,
        ),
    ];
    let mut ok = true;
    for (name, part, total, floor) in metrics {
        let percentage = pct(part, total);
        println!("{spec} {name}: {part}/{total} ({percentage:.1}%), floor {floor:.0}%");
        if percentage < floor {
            println!("FLOOR {spec} {name}");
            ok = false;
        }
    }
    ok
}

/// The untyped `To`-form parameters for review: algorithm, parameter, type text.
fn print_untyped(spec: &str, state: &StateSpec) {
    for signature in state
        .signatures
        .iter()
        .filter(|s| s.form == SignatureForm::To)
    {
        for param in signature.params.iter().filter(|p| !is_typed(p)) {
            println!(
                "  untyped: {spec}#{} {} [{}]",
                signature.algorithm.anchor, param.name, param.type_text
            );
        }
    }
}

fn index(html: &str, dom: &str, catalog: &StateCatalog) -> (rusqlite::Connection, [i64; 2]) {
    let conn = webspec_index::db::open_in_memory().unwrap();
    let h = webspec_index::state::testing::index_offline_with(
        &conn,
        "HTML",
        "https://html.spec.whatwg.org/",
        html,
        catalog,
    )
    .unwrap();
    let d = webspec_index::state::testing::index_offline_with(
        &conn,
        "DOM",
        "https://dom.spec.whatwg.org/",
        dom,
        catalog,
    )
    .unwrap();
    (conn, [h, d])
}

fn model(conn: &rusqlite::Connection, snapshot: i64) -> StateSpec {
    webspec_index::db::state::load_state_model(conn, snapshot)
        .unwrap()
        .unwrap()
}

fn ratio(part: u32, whole: u32) -> String {
    format!(
        "{part}/{whole} ({:.1}%)",
        100.0 * part as f64 / whole.max(1) as f64
    )
}

/// The §6.4–§6.6 statement measurements. The `Let`/`Return` rates cover every
/// source; the §6.5 probe read algorithm steps only, so the step-only rates
/// follow. The §6.6 probe checked `To` algorithms only, so their count follows
/// the all-subject one.
fn print_statements(state: &StateSpec) {
    let c = &state.coverage;
    let mut heads: Vec<(&String, &u32)> = c.step_heads.iter().collect();
    heads.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let top: Vec<String> = heads
        .iter()
        .take(10)
        .map(|(tag, n)| format!("{tag} {n}"))
        .collect();
    println!(
        "  step heads recognized {}; top: {}",
        ratio(c.steps_recognized, c.steps),
        top.join(", ")
    );
    println!(
        "  If parsed {}, Assert parsed {}, For each bound {}",
        ratio(c.if_parsed, c.if_total),
        ratio(c.assert_parsed, c.assert_total),
        ratio(c.foreach_bound, c.foreach_total)
    );
    let opaque = |slot: &str| {
        let prefix = format!("{slot}:");
        let total: u32 = c
            .expr_forms
            .iter()
            .filter(|(form, _)| form.starts_with(&prefix))
            .map(|(_, n)| n)
            .sum();
        let opaque = c
            .expr_forms
            .get(&format!("{slot}:opaque"))
            .copied()
            .unwrap_or(0);
        ratio(opaque, total)
    };
    println!(
        "  Let opaque {}, Return opaque {}; expr forms {:?}",
        opaque("let"),
        opaque("return"),
        c.expr_forms
    );
    let step_sources: HashSet<&str> = state
        .sources
        .iter()
        .filter(|s| matches!(s.context, SourceContext::Algorithm { .. }))
        .map(|s| s.id.as_str())
        .collect();
    let (mut lets, mut returns) = ((0, 0), (0, 0));
    for statement in &state.statements {
        if !step_sources.contains(statement.source_id.as_str()) {
            continue;
        }
        let (count, value) = match &statement.kind {
            StatementKind::Let { value, .. } => (&mut lets, Some(value)),
            StatementKind::Return { value } => (&mut returns, value.as_ref()),
            _ => continue,
        };
        count.0 += u32::from(matches!(value, Some(Expr::Opaque { .. })));
        count.1 += 1;
    }
    println!(
        "  in algorithm steps: Let opaque {}, Return opaque {}",
        ratio(lets.0, lets.1),
        ratio(returns.0, returns.1)
    );
    let algorithms: BTreeSet<&str> = c
        .undeclared_review
        .iter()
        .map(|item| item.subject.as_str())
        .collect();
    let to_algorithms: HashSet<&AnchorTarget> = state
        .signatures
        .iter()
        .filter(|s| s.form == SignatureForm::To)
        .map(|s| &s.algorithm)
        .collect();
    let (mut to_undeclared, mut to_with_any) = (0, 0);
    for origins in state
        .var_origins
        .iter()
        .filter(|o| to_algorithms.contains(&o.subject))
    {
        let n = origins
            .vars
            .iter()
            .filter(|v| v.origin == Origin::Undeclared)
            .count();
        to_undeclared += n;
        to_with_any += usize::from(n > 0);
    }
    println!(
        "  undeclared vars {} in {} algorithms (To algorithms: {to_undeclared} in {to_with_any} of {}); calls by form {:?}",
        c.undeclared_vars,
        algorithms.len(),
        to_algorithms.len(),
        c.calls_by_form
    );
}

/// Review items; their subject already carries the spec.
fn print_review(label: &str, items: &[ReviewItem]) {
    for item in items {
        println!(
            "  {label}: {}{} [{}] {}",
            item.subject,
            item.step_path
                .as_deref()
                .map(|p| format!(":{p}"))
                .unwrap_or_default(),
            item.target,
            item.text
        );
    }
}

/// Prints the §12.4 numbers for one run; returns false when a floor is violated.
fn report(run: &str, spec: &str, state: &StateSpec, floors: &Floors) -> bool {
    let (c, signatures) = (&state.coverage, &state.signatures);
    let owned: u32 = c.written_fields_owned.values().sum();
    let owner_pct = 100.0 * owned as f64 / c.written_fields.max(1) as f64;
    let set_pct = 100.0 * c.set_structured as f64 / c.set_total.max(1) as f64;
    let occ: u32 = c.occurrences.values().sum();
    let unclassified_pct =
        100.0 * *c.occurrences.get("unclassified").unwrap_or(&0) as f64 / occ.max(1) as f64;
    println!(
        "{spec} ({run}): written-field owners {owner_pct:.1}% ({owned}/{}), structured Set {set_pct:.1}% ({}/{}), unclassified {unclassified_pct:.2}% of {occ}",
        c.written_fields, c.set_structured, c.set_total
    );
    println!(
        "  concept dfns {}, owner candidates {} (resolved {}), by rule {:?}",
        c.concept_dfns, c.owner_candidates, c.owner_resolved, c.owner_by_rule
    );
    println!(
        "  written-field owners by rule {:?}",
        c.written_fields_owned
    );
    println!("  statements {:?}", c.statements);
    println!("  occurrences {:?}", c.occurrences);
    let pct = |part: u32, whole: u32| 100.0 * part as f64 / whole.max(1) as f64;
    let params: u32 = c.to_params.values().sum();
    let typed: u32 = ["explicit", "name_resolved", "dfn_for"]
        .iter()
        .filter_map(|basis| c.to_params.get(*basis))
        .sum();
    println!(
        "  signatures: template {}/{} ({:.1}%), forms {:?}, To params typed {:.1}%, IDL from IDL {}/{}",
        c.template_signatures,
        c.algorithms,
        pct(c.template_signatures, c.algorithms),
        c.intro_forms,
        pct(typed, params),
        c.idl_from_idl,
        c.idl_signatures
    );
    // §6.2 counts IDL signatures as "with template" and types `To` intros only.
    let with_idl = c.template_signatures + c.idl_signatures;
    let (to_typed, to_params) = signatures
        .iter()
        .filter(|s| s.form == SignatureForm::To)
        .flat_map(|s| &s.params)
        .fold((0, 0), |(typed, all), p| {
            (typed + u32::from(is_typed(p)), all + 1)
        });
    println!(
        "  signatures (§6.2 terms): template or IDL {with_idl}/{} ({:.1}%), To-form params typed {to_typed}/{to_params} ({:.1}%)",
        c.algorithms,
        pct(with_idl, c.algorithms),
        pct(to_typed, to_params)
    );
    print_statements(state);
    for unresolved in &c.written_fields_unresolved {
        println!("  unresolved owner: {spec}#{unresolved}");
    }
    let ok = owner_pct >= floors.owner
        && set_pct >= floors.set
        && unclassified_pct <= floors.unclassified;
    if !ok {
        println!("FLOOR {spec} ({run}) below §12.4");
    }
    let to_typed_ok = pct(to_typed, to_params) >= floors.to_typed;
    if !to_typed_ok {
        println!("FLOOR {spec} ({run}) To params typed below §14.5");
    }
    ok && to_typed_ok
}

/// Exact / partial / unbound counts of one population of calls.
#[derive(Default)]
struct Rates {
    exact: u32,
    partial: u32,
    unbound: u32,
}

impl Rates {
    fn add(&mut self, confidence: Confidence) {
        match confidence {
            Confidence::Exact => self.exact += 1,
            Confidence::Partial => self.partial += 1,
            Confidence::Unbound => self.unbound += 1,
        }
    }

    fn exact_pct(&self) -> f64 {
        let n = self.exact + self.partial + self.unbound;
        100.0 * self.exact as f64 / n.max(1) as f64
    }

    fn line(&self) -> String {
        let n = self.exact + self.partial + self.unbound;
        let pct = |part: u32| 100.0 * part as f64 / n.max(1) as f64;
        format!(
            "{n}, exact {:.1}%, partial {:.1}%, unbound {:.1}%",
            pct(self.exact),
            pct(self.partial),
            pct(self.unbound)
        )
    }
}

/// §14.5 floor on exact bindings of calls to `To` targets with parameters, checked over the
/// `To`-like targets: `when the steps say` and given-list intros are call templates parsed
/// exactly like `To` (§8.1.6), and `bind` treats all three alike.
fn binding_floor(spec: &str) -> f64 {
    if spec == "HTML" {
        70.0
    } else {
        80.0
    }
}

/// §13 budget for binding every call of HTML.
const HTML_BIND_BUDGET_MS: f64 = 20.0;

/// A signature whose intro is a call template and that takes parameters.
fn is_to_like(signature: &Signature) -> bool {
    matches!(
        signature.form,
        SignatureForm::To | SignatureForm::WhenStepsSay | SignatureForm::GivenList
    ) && !signature.params.is_empty()
}

/// One non-exact binding for review: caller, callee, template, issues, argument region.
fn print_binding(stored: &StoredCall, signature: &Signature, binding: &Binding) {
    let region = &stored.call.region;
    let text = stored
        .source
        .text
        .get(region.start..region.end)
        .unwrap_or_default();
    let template: Vec<String> = signature
        .template
        .iter()
        .flat_map(|t| &t.pieces)
        .map(|piece| match piece {
            TemplatePiece::Head(head) | TemplatePiece::Literal(head) => head.clone(),
            TemplatePiece::Callee => "CALLEE".into(),
            TemplatePiece::Slot(i) => format!("${i}"),
            TemplatePiece::ListSep => ",".into(),
            TemplatePiece::NamedGroup(group) => format!("[{group}]"),
        })
        .collect();
    let issues: Vec<String> = binding
        .issues
        .iter()
        .map(|issue| format!("{issue:?}"))
        .collect();
    println!(
        "  binding {:?}: {}:{}{} -> {}#{} `{}` {} :: {text}",
        binding.confidence,
        stored.spec,
        stored.subject,
        stored
            .step_path
            .as_deref()
            .map(|p| format!(":{p}"))
            .unwrap_or_default(),
        binding.callee.spec,
        binding.callee.anchor,
        template.join(" "),
        issues.join(", ")
    );
}

/// §6.3 / §14.5 binding rates and the §13 binding time over the stored calls of both
/// snapshots; returns false when a floor or the budget is violated. `To`-like means the
/// forms whose intro is a call template (`To`, `when the steps say`, given-list), as in
/// `bind`. Over all calls, a call without a target or signature counts as unbound.
fn report_bindings(conn: &rusqlite::Connection, snapshots: [i64; 2], review: bool) -> bool {
    let calls: Vec<Vec<StoredCall>> = snapshots
        .iter()
        .map(|&s| webspec_index::db::state::calls_of_snapshot(conn, s).unwrap())
        .collect();
    let targets: Vec<AnchorTarget> = calls
        .iter()
        .flatten()
        .filter_map(|c| c.call.callee.target.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let signatures =
        webspec_index::db::state::signatures_for_targets(conn, &snapshots, &targets).unwrap();
    let mut ok = true;
    for (spec, calls) in ["HTML", "DOM"].into_iter().zip(&calls) {
        let with_signature: Vec<(&StoredCall, &Signature)> = calls
            .iter()
            .filter_map(|c| {
                let target = c.call.callee.target.as_ref()?;
                Some((c, signatures.get(target)?))
            })
            .collect();
        let start = std::time::Instant::now();
        let bindings: Vec<Binding> = with_signature
            .iter()
            .map(|(stored, signature)| bind(&stored.call, &stored.source, signature))
            .collect();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let (mut to_like, mut to, mut all) = (Rates::default(), Rates::default(), Rates::default());
        for ((_, signature), binding) in with_signature.iter().zip(&bindings) {
            all.add(binding.confidence);
            if is_to_like(signature) {
                to_like.add(binding.confidence);
                if signature.form == SignatureForm::To {
                    to.add(binding.confidence);
                }
            }
        }
        all.unbound += (calls.len() - with_signature.len()) as u32;
        println!(
            "{spec} bindings: calls to To-like targets with parameters: {}",
            to_like.line()
        );
        println!("  of which `To` targets: {}", to.line());
        println!(
            "  all calls: {} ({} without a target signature)",
            all.line(),
            calls.len() - with_signature.len()
        );
        println!("  bound {} calls in {ms:.2} ms", bindings.len());
        if to_like.exact_pct() < binding_floor(spec) {
            println!("FLOOR {spec} exact bindings of calls to To-like targets below §14.5");
            ok = false;
        }
        if spec == "HTML" && ms >= HTML_BIND_BUDGET_MS {
            println!("BUDGET {spec} binding took {ms:.2} ms, §13 allows {HTML_BIND_BUDGET_MS} ms");
            ok = false;
        }
        if review {
            for ((stored, signature), binding) in with_signature.iter().zip(&bindings) {
                if binding.confidence != Confidence::Exact && is_to_like(signature) {
                    print_binding(stored, signature, binding);
                }
            }
        }
    }
    ok
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let review = args.iter().any(|a| a == "--review");
    args.retain(|a| a != "--review");
    let [html, dom] = args.as_slice() else {
        eprintln!("usage: state_golden [--review] HTML.html DOM.html");
        std::process::exit(2)
    };
    let html = std::fs::read_to_string(html).unwrap();
    let dom = std::fs::read_to_string(dom).unwrap();
    let mut exit = 0;

    let start = std::time::Instant::now();
    let (conn, snapshots) = index(
        &html,
        &dom,
        webspec_index::state::extract::bundled_catalog(),
    );
    eprintln!("indexed (bundled) in {:?}", start.elapsed());
    for f in golden::check(&conn) {
        println!("GOLDEN {f}");
        exit = 1;
    }
    let ir = ir_golden::check(&conn);
    for failure in ir.failures {
        println!("GOLDEN {failure}");
        exit = 1;
    }
    println!("GAPS {}", ir.gaps.len());
    for (spec, snapshot) in ["HTML", "DOM"].into_iter().zip(snapshots) {
        let state = model(&conn, snapshot);
        if !report_ir_floors(spec, &state) {
            exit = 1;
        }
        if !report("bundled", spec, &state, &bundled_floors(spec)) {
            exit = 1;
        }
        if review {
            let c = &state.coverage;
            print_review("review", &c.unclassified_review);
            print_review("assert", &c.assert_review);
            print_review("undeclared", &c.undeclared_review);
            print_untyped(spec, &state);
        }
    }
    if !report_bindings(&conn, snapshots, review) {
        exit = 1;
    }

    let (conn, snapshots) = index(&html, &dom, &StateCatalog::default());
    for (spec, snapshot) in ["HTML", "DOM"].into_iter().zip(snapshots) {
        if !report(
            "grammar",
            spec,
            &model(&conn, snapshot),
            &grammar_floors(spec),
        ) {
            exit = 1;
        }
    }
    std::process::exit(exit);
}
