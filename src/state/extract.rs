//! `extract_state` (§9.1): the object model, statement sources, statements and
//! occurrences of one snapshot, and the rows derived from them (§9.2).
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::model::{ParsedIdlDefinition, ParsedSection};
use crate::parse::steps::{
    AnchorTarget, InlineTokenKind, LinkSpan, ReferenceRole, StepItem, StructuralBranch,
    StructuralSpec, TextSpan,
};
use crate::state::catalog::load_state_files;
use crate::state::declare;
use crate::state::grammar::{Callable, Env};
use crate::state::idl_sig::{idl_signature, IdlMembers};
use crate::state::intro::{algorithm_intros, prose_intro, IdIndex};
use crate::state::ir::{
    self, Call, CallForm, Expr, Hop, InitForm, MutationOp, OpBasis, OpaqueReason, Origin,
    ParsedSource, Path, Predicate, ProseRole, Root, SetForm, SourceContext, Statement,
    StatementKind, StatementSource, VarOrigins,
};
use crate::state::model::{
    AlgorithmSummary, CoverageCounters, FieldDef, Literal, ObjectModel, Occurrence,
    OccurrenceClass, Owner, Primitive, Reflection, ReturnBasis, ReturnType, ReviewItem, Signature,
    SignatureForm, SignatureIssue, Site, SiteClass, StateCatalog, StateSpec, TypeBasis, TypeExpr,
    TypeKey, TypeRef,
};
use crate::state::names::NameResolver;
use crate::state::signature::extract_signatures;
use crate::state::tree::link_tree;
use crate::state::vars::{uses, var_origins, IrIndex};
use crate::state::{classify, prose, reflect, roles, rules, types};

/// Everything `extract_state` reads. The caller parses the document once and
/// shares it with the section and structural extraction.
pub struct StateInputs<'a> {
    pub document: &'a scraper::Html,
    pub spec: &'a str,
    pub base_url: &'a str,
    pub snapshot_sha: &'a str,
    pub structure: &'a StructuralSpec,
    pub sections: &'a [ParsedSection],
    pub idl_definitions: &'a [ParsedIdlDefinition],
    pub catalog: &'a StateCatalog,
    /// Pre-computed set of algorithm-body node IDs (from
    /// `structural_body_nodes`).  When `Some`, passed directly to
    /// `prose_sources` so `find_algorithm_candidates` is not run a second
    /// time on the same document.  `None` falls back to computing it inline.
    pub body_nodes: Option<HashSet<ego_tree::NodeId>>,
}

/// The reviewed catalog applied at index time: the `state/` files of the embedded
/// semantics package.
pub fn bundled_catalog() -> &'static StateCatalog {
    static CATALOG: OnceLock<StateCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        load_state_files(crate::semantics::bundled::FILES).expect("bundled state catalog is valid")
    })
}

/// The `representation_version` of a snapshot indexed with the bundled catalog.
pub fn bundled_representation_version() -> String {
    bundled_catalog().representation_version()
}

/// Pure: no DB access, no network.
pub fn extract_state(inputs: &StateInputs) -> StateSpec {
    let StateInputs {
        document,
        spec,
        base_url,
        structure,
        ..
    } = *inputs;
    let concepts = declare::concept_dfns(document);
    let mut table =
        types::collect_types(document, spec, base_url, inputs.idl_definitions, &concepts);
    let declared = declare::declare_with_catalog(
        document,
        spec,
        base_url,
        structure,
        &mut table,
        inputs.catalog,
    );
    let reflections =
        reflect::reflections(document, spec, base_url, inputs.idl_definitions, &table);
    let model = ObjectModel {
        types: table.types.into_values().collect(),
        fields: declared.fields,
        members: declared.members,
        reflections,
    };

    let names = NameResolver::new(spec, &model, &concepts);
    let ids = IdIndex::new(document);
    let intros = algorithm_intros(
        document,
        &ids,
        spec,
        base_url,
        inputs.snapshot_sha,
        structure,
    );
    let members = IdlMembers::new(inputs.idl_definitions, &model);
    let mut signatures = extract_signatures(&intros, &names, &members);

    let (mut sources, mut branch_inits) = algorithm_sources(structure);
    let algorithm_ranges = algorithm_source_ranges(structure, &sources);
    sources.extend(intros.into_iter().map(|intro| intro.source));
    let prose = prose::prose_sources(
        document,
        spec,
        base_url,
        inputs.snapshot_sha,
        inputs.sections,
        inputs.body_nodes.as_ref(),
    );
    add_prose_signatures(&prose.sources, &ids, &members, &mut signatures);
    let prose_sources = prose.sources.len() as u32;
    sources.extend(prose.sources);

    let env = call_env(spec, structure, &signatures);
    let mut owner = vec![None; sources.len()];
    for (algorithm, range) in algorithm_ranges.iter().enumerate() {
        owner[range.clone()].fill(Some(algorithm));
    }
    let mut algorithm_statements: Vec<Vec<Statement>> = vec![Vec::new(); algorithm_ranges.len()];
    let mut other_statements = Vec::new();
    let mut calls = Vec::new();
    let mut link_roles = Vec::with_capacity(sources.len());
    let mut occurrences = Vec::new();
    let mut declared_sites = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        if matches!(source.context, SourceContext::Intro { .. }) {
            link_roles.push(roles::link_roles(source, &ParsedSource::default(), &[]));
            continue;
        }
        let mut parsed = match branch_inits.remove(&index) {
            Some((constructed, value)) => ir::parse_branch_label(source, constructed, value),
            None => ir::parse_source_with(source, &env),
        };
        let mut source_occurrences = classify::classify(source, &parsed);
        rules::apply_rules(
            inputs.catalog,
            source,
            &mut parsed,
            &mut source_occurrences,
            &mut declared_sites,
        );
        link_roles.push(roles::link_roles(source, &parsed, &source_occurrences));
        occurrences.extend(source_occurrences);
        calls.extend(parsed.calls);
        match owner[index] {
            Some(algorithm) => algorithm_statements[algorithm].extend(parsed.statements),
            None => other_statements.extend(parsed.statements),
        }
    }

    let mut issues = declared.issues;
    for ((algorithm, range), statements) in structure
        .algorithms
        .iter()
        .zip(&algorithm_ranges)
        .zip(&mut algorithm_statements)
    {
        link_tree(algorithm, &sources[range.clone()], statements, &mut issues);
    }
    let statements: Vec<Statement> = algorithm_statements
        .into_iter()
        .flatten()
        .chain(other_statements)
        .collect();

    let counters = declared.counters;
    let mut coverage = CoverageCounters {
        concept_dfns: counters.concept_dfns,
        owner_by_rule: counters.owner_by_rule,
        owner_candidates: counters.owner_candidates,
        owner_resolved: counters.owner_resolved,
        set_members: counters.set_members,
        prose_sources,
        prose_callouts_excluded: prose.callouts_excluded,
        prose_mentions: prose.mentions.values().sum(),
        ..CoverageCounters::default()
    };
    count_signatures(structure, &signatures, &mut coverage);
    let concept_ids: HashSet<&str> = concepts.iter().map(|c| c.id.as_str()).collect();
    count_occurrences(
        spec,
        &model.fields,
        &concept_ids,
        &sources,
        &occurrences,
        &mut coverage,
    );

    let mut state = StateSpec {
        representation_version: inputs.catalog.representation_version(),
        spec: spec.to_string(),
        snapshot_sha: inputs.snapshot_sha.to_string(),
        model,
        sources,
        statements,
        occurrences,
        prose_mentions: prose.mentions,
        coverage: CoverageCounters::default(),
        issues,
        declared_sites,
        signatures,
        calls,
        link_roles,
        ..Default::default()
    };

    let index = IrIndex::new(&state);
    let returns = return_types(&state.signatures, &index);
    let var_origins = subject_var_origins(&state.signatures, &index);
    let summaries = summaries(structure, &state.sources, &var_origins, &index);
    count_statements(&state.statements, &index, &mut coverage);
    count_ir(&state, structure, &var_origins, &index, &mut coverage);
    drop(index);

    for (at, returns) in returns {
        state.signatures[at].returns = Some(returns);
    }
    state.var_origins = var_origins;
    state.summaries = summaries;
    state.coverage = coverage;
    state
}

/// The statement grammar's view of this spec (§9.1): what each in-spec
/// anchor is callable as, and the structural operation sites that mention an
/// algorithm or pass it bodies.
fn call_env(spec: &str, structure: &StructuralSpec, signatures: &[Signature]) -> Env {
    let mut env = Env {
        spec: spec.to_string(),
        ..Env::default()
    };
    for signature in signatures {
        let callable = match (&signature.form, &signature.template) {
            (SignatureForm::Accessor, _) => Callable::Accessor,
            (SignatureForm::Predicate, _) => Callable::Predicate,
            (_, Some(_)) => Callable::Template,
            (_, None) => Callable::NoTemplate,
        };
        env.callables
            .entry(signature.algorithm.anchor.clone())
            .or_insert(callable);
    }
    for algorithm in &structure.algorithms {
        env.callables
            .entry(algorithm.source.section_anchor.clone())
            .or_insert(Callable::Body);
        for site in &algorithm.operation_sites {
            let key = (site.segment_id.clone(), site.link_id.clone());
            if site.role == ReferenceRole::Mention {
                env.mentions.insert(key.clone());
            }
            if !site.actual_body_ids.is_empty() {
                env.body_args.insert(key, site.actual_body_ids.clone());
            }
        }
    }
    env
}

/// The range of `sources` (from [`algorithm_sources`]) each structural
/// algorithm owns: its segments and its `<dl>` initializer entries.
fn algorithm_source_ranges(
    structure: &StructuralSpec,
    sources: &[StatementSource],
) -> Vec<Range<usize>> {
    let mut at = 0;
    structure
        .algorithms
        .iter()
        .map(|algorithm| {
            let segments = algorithm.segments.len();
            let labels = sources[at + segments..]
                .iter()
                .take_while(|source| {
                    matches!(&source.context, SourceContext::BranchLabel { .. })
                        && source.subject.anchor == algorithm.source.section_anchor
                })
                .count();
            let range = at..at + segments + labels;
            at = range.end;
            range
        })
        .collect()
}

/// Return types from `Return` statements (§8.1.1) for signatures without
/// one: every value `Return` a literal of one primitive kind, possibly with
/// `null`. Positions into `signatures`.
fn return_types(signatures: &[Signature], index: &IrIndex) -> Vec<(usize, ReturnType)> {
    signatures
        .iter()
        .enumerate()
        .filter(|(_, signature)| signature.returns.is_none())
        .filter_map(|(at, signature)| {
            let statements = index.statements_by_subject.get(&signature.algorithm)?;
            let ty = literal_return_type(statements)?;
            Some((
                at,
                ReturnType {
                    ty,
                    basis: ReturnBasis::ReturnStatements,
                },
            ))
        })
        .collect()
}

fn literal_return_type(statements: &[&Statement]) -> Option<TypeExpr> {
    let mut primitive = None;
    let mut null = false;
    for statement in statements {
        let StatementKind::Return { value: Some(value) } = &statement.kind else {
            continue;
        };
        let kind = match value {
            Expr::Literal(Literal::Null) => {
                null = true;
                continue;
            }
            Expr::Literal(Literal::Bool(_)) => Primitive::Boolean,
            Expr::Literal(Literal::String(_)) => Primitive::String,
            Expr::Literal(Literal::Number(_)) => Primitive::Number,
            _ => return None,
        };
        if primitive.is_some_and(|seen| seen != kind) {
            return None;
        }
        primitive = Some(kind);
    }
    let primitive = TypeExpr::Primitive(primitive?);
    Some(if null {
        TypeExpr::Union(vec![primitive, TypeExpr::Null])
    } else {
        primitive
    })
}

/// Variable origins of every subject with statements, in subject order.
fn subject_var_origins(signatures: &[Signature], index: &IrIndex) -> Vec<VarOrigins> {
    let mut by_subject: HashMap<&AnchorTarget, &Signature> = HashMap::new();
    for signature in signatures {
        by_subject.entry(&signature.algorithm).or_insert(signature);
    }
    index
        .statements_by_subject
        .keys()
        .map(|subject| var_origins(subject, by_subject.get(subject).copied(), index))
        .collect()
}

/// One summary per structural algorithm and per IDL-role prose subject.
fn summaries(
    structure: &StructuralSpec,
    sources: &[StatementSource],
    var_origins: &[VarOrigins],
    index: &IrIndex,
) -> Vec<AlgorithmSummary> {
    let origins: HashMap<&AnchorTarget, &VarOrigins> =
        var_origins.iter().map(|o| (&o.subject, o)).collect();
    let algorithms = structure.algorithms.iter().map(|algorithm| AnchorTarget {
        spec: structure.spec.clone(),
        anchor: algorithm.source.section_anchor.clone(),
    });
    let idl_steps = sources
        .iter()
        .filter(|source| {
            matches!(
                source.context,
                SourceContext::Prose {
                    role: ProseRole::Getter
                        | ProseRole::Setter
                        | ProseRole::Method
                        | ProseRole::Constructor,
                    ..
                }
            )
        })
        .map(|source| source.subject.clone());
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for subject in algorithms.chain(idl_steps) {
        if !seen.insert(subject.clone()) {
            continue;
        }
        let statements = index
            .statements_by_subject
            .get(&subject)
            .map_or(&[][..], Vec::as_slice);
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        let mut opaque = Vec::new();
        for statement in statements {
            *counts
                .entry(kind_tag(&statement.kind).to_string())
                .or_default() += 1;
            if matches!(statement.kind, StatementKind::Opaque { .. }) {
                opaque.push(review_item(index, statement, String::new()));
            }
        }
        let calls = index
            .sources_by_subject
            .get(&subject)
            .into_iter()
            .flatten()
            .filter_map(|source| index.calls_by_source.get(source.id.as_str()))
            .map(Vec::len)
            .sum::<usize>() as u32;
        out.push(AlgorithmSummary {
            anchor: subject.anchor.clone(),
            statements: counts,
            opaque,
            var_origins: origins
                .get(&subject)
                .map(|o| o.vars.clone())
                .unwrap_or_default(),
            calls,
        });
    }
    out
}

/// A review item for `statement`: its subject, step and span text.
fn review_item(index: &IrIndex, statement: &Statement, target: String) -> ReviewItem {
    let source = index.sources.get(statement.source_id.as_str()).copied();
    ReviewItem {
        subject: source.map_or_else(String::new, |s| anchor_key(&s.subject)),
        step_path: source
            .and_then(|s| step_path(&s.context))
            .map(str::to_string),
        target,
        text: source
            .and_then(|s| s.text.get(statement.span.start..statement.span.end))
            .unwrap_or_default()
            .to_string(),
    }
}

/// Length of the step prefix (`⌛ `, `Optionally, `) a step head may follow;
/// the head rule of the statement tree (§8.2).
fn head_prefix_len(text: &str) -> usize {
    let rest = text.strip_prefix("\u{231B} ").unwrap_or(text);
    let rest = rest
        .strip_prefix("Optionally, ")
        .or_else(|| rest.strip_prefix("Optionally "))
        .unwrap_or(rest);
    text.len() - rest.len()
}

/// Statement-level coverage (§14.5): step heads, conditions, assertions,
/// loop variables, `Let`/`Return` value forms, call forms and undeclared
/// variables.
fn count_ir(
    state: &StateSpec,
    structure: &StructuralSpec,
    var_origins: &[VarOrigins],
    index: &IrIndex,
    coverage: &mut CoverageCounters,
) {
    let mut first_statement: HashMap<&str, &Statement> = HashMap::new();
    for statements in index.statements_by_subject.values() {
        for &statement in statements {
            first_statement
                .entry(statement.source_id.as_str())
                .or_insert(statement);
        }
    }
    for step in structure.algorithms.iter().flat_map(|a| &a.steps) {
        coverage.steps += 1;
        let source = step
            .items
            .iter()
            .find_map(|item| match item {
                StepItem::Segment(id) => Some(id.as_str()),
                _ => None,
            })
            .and_then(|id| index.sources.get(id));
        let head = source.and_then(|source| {
            first_statement
                .get(source.id.as_str())
                .filter(|head| head.span.start <= head_prefix_len(&source.text))
        });
        // A `For each` head (plain or a linked list-iterate, whose text is
        // the same) the loop grammar cannot parse is an unbound loop.
        let loop_head = source.is_some_and(|source| {
            source.text[head_prefix_len(&source.text)..].starts_with("For each ")
        });
        if loop_head && !head.is_some_and(|head| matches!(head.kind, StatementKind::ForEach { .. }))
        {
            coverage.foreach_total += 1;
        }
        let tag = match head {
            Some(head) => {
                if !matches!(head.kind, StatementKind::Opaque { .. }) {
                    coverage.steps_recognized += 1;
                }
                kind_tag(&head.kind)
            }
            None => "unrecognized",
        };
        *coverage.step_heads.entry(tag.to_string()).or_default() += 1;
    }

    let mut expr_form = |slot: &str, form: &str| {
        *coverage
            .expr_forms
            .entry(format!("{slot}:{form}"))
            .or_default() += 1;
    };
    for statement in &state.statements {
        match &statement.kind {
            StatementKind::Let { value, .. } => expr_form("let", expr_form_name(value)),
            StatementKind::Return { value } => {
                expr_form("return", value.as_ref().map_or("none", expr_form_name))
            }
            _ => {}
        }
    }
    for statement in &state.statements {
        match &statement.kind {
            StatementKind::If { condition, .. }
            | StatementKind::Otherwise {
                condition: Some(condition),
                ..
            } => {
                coverage.if_total += 1;
                if !matches!(condition, Predicate::Opaque { .. }) {
                    coverage.if_parsed += 1;
                }
            }
            StatementKind::Assert { predicate } => {
                coverage.assert_total += 1;
                if matches!(predicate, Predicate::Opaque { .. }) {
                    coverage
                        .assert_review
                        .push(review_item(index, statement, String::new()));
                } else {
                    coverage.assert_parsed += 1;
                }
            }
            StatementKind::ForEach { vars, .. } => {
                coverage.foreach_total += 1;
                if !vars.is_empty() {
                    coverage.foreach_bound += 1;
                }
            }
            _ => {}
        }
    }
    for call in &state.calls {
        *coverage
            .calls_by_form
            .entry(call_form_name(call.form).to_string())
            .or_default() += 1;
    }

    for origins in var_origins {
        let undeclared: Vec<&str> = origins
            .vars
            .iter()
            .filter(|v| v.origin == Origin::Undeclared)
            .map(|v| v.name.as_str())
            .collect();
        if undeclared.is_empty() {
            continue;
        }
        coverage.undeclared_vars += undeclared.len() as u32;
        let mut first_use: HashMap<&str, &Statement> = HashMap::new();
        for &statement in index
            .statements_by_subject
            .get(&origins.subject)
            .into_iter()
            .flatten()
        {
            for name in uses(statement, index) {
                first_use.entry(name).or_insert(statement);
            }
        }
        for name in undeclared {
            coverage.undeclared_review.push(match first_use.get(name) {
                Some(statement) => review_item(index, statement, name.to_string()),
                None => ReviewItem {
                    subject: anchor_key(&origins.subject),
                    step_path: None,
                    target: name.to_string(),
                    text: String::new(),
                },
            });
        }
    }
}

fn expr_form_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::Var(_) => "var",
        Expr::This => "this",
        Expr::Literal(_) => "literal",
        Expr::Path(_) => "path",
        Expr::New { .. } => "new",
        Expr::Call(_) => "call",
        Expr::AlgorithmRef { .. } => "algorithm_ref",
        Expr::List(_) => "list",
        Expr::EnumValue { .. } => "enum",
        Expr::Conditional { .. } => "conditional",
        Expr::Opaque { .. } => "opaque",
    }
}

fn call_form_name(form: CallForm) -> &'static str {
    match form {
        CallForm::Imperative => "imperative",
        CallForm::ResultOf => "result_of",
        CallForm::Gerund => "gerund",
        CallForm::Possessive => "possessive",
        CallForm::Predicate => "predicate",
        CallForm::SetToBe => "set_to_be",
        CallForm::Ecmarkup => "ecmarkup",
    }
}

/// The IDL signature of each getter, setter, method or constructor prose
/// source whose subject has none yet (§8.1.2).
fn add_prose_signatures(
    sources: &[StatementSource],
    ids: &IdIndex,
    members: &IdlMembers,
    signatures: &mut Vec<Signature>,
) {
    let mut signed: HashSet<String> = signatures
        .iter()
        .map(|signature| signature.algorithm.anchor.clone())
        .collect();
    for source in sources {
        let SourceContext::Prose { role, .. } = source.context else {
            continue;
        };
        if !matches!(
            role,
            ProseRole::Getter | ProseRole::Setter | ProseRole::Method | ProseRole::Constructor
        ) || signed.contains(&source.subject.anchor)
        {
            continue;
        }
        let Some(signature) =
            prose_intro(ids, source, role).and_then(|intro| idl_signature(&intro, members))
        else {
            continue;
        };
        signed.insert(source.subject.anchor.clone());
        signatures.push(signature);
    }
}

/// Signature coverage (§6.2) of structural algorithms: forms, templates,
/// `To`-like parameter type bases and IDL signatures found in the IDL.
/// Signatures of prose IDL steps are not algorithms and are not counted.
fn count_signatures(
    structure: &StructuralSpec,
    signatures: &[Signature],
    coverage: &mut CoverageCounters,
) {
    let algorithms: HashSet<&str> = structure
        .algorithms
        .iter()
        .map(|algorithm| algorithm.source.section_anchor.as_str())
        .collect();
    let signatures: Vec<&Signature> = signatures
        .iter()
        .filter(|signature| algorithms.contains(signature.algorithm.anchor.as_str()))
        .collect();
    let signed: HashSet<&str> = signatures
        .iter()
        .map(|signature| signature.algorithm.anchor.as_str())
        .collect();
    coverage.algorithms = structure.algorithms.len() as u32;
    let unsigned = structure
        .algorithms
        .iter()
        .filter(|algorithm| !signed.contains(algorithm.source.section_anchor.as_str()))
        .count() as u32;
    if unsigned > 0 {
        coverage.intro_forms.insert("none".to_string(), unsigned);
    }
    for signature in signatures {
        *coverage
            .intro_forms
            .entry(signature.form.as_str().to_string())
            .or_default() += 1;
        if signature.template.is_some() {
            coverage.template_signatures += 1;
        }
        match &signature.form {
            SignatureForm::To | SignatureForm::WhenStepsSay | SignatureForm::GivenList => {
                for param in &signature.params {
                    let basis = match (param.type_basis, &param.ty) {
                        (TypeBasis::Explicit, TypeExpr::Opaque { .. }) => "opaque",
                        (TypeBasis::Explicit, _) => "explicit",
                        (TypeBasis::NameResolved, _) => "name_resolved",
                        (TypeBasis::DfnFor, _) => "dfn_for",
                        (TypeBasis::Unknown, _) => "unknown",
                    };
                    *coverage.to_params.entry(basis.to_string()).or_default() += 1;
                }
            }
            SignatureForm::IdlMethod { .. }
            | SignatureForm::IdlGetter { .. }
            | SignatureForm::IdlSetter { .. }
            | SignatureForm::IdlConstructor { .. } => {
                coverage.idl_signatures += 1;
                if !signature
                    .issues
                    .contains(&SignatureIssue::IdlMemberNotFound)
                {
                    coverage.idl_from_idl += 1;
                }
            }
            _ => {}
        }
    }
}

fn set_form_name(form: SetForm) -> &'static str {
    match form {
        SetForm::To => "to",
        SetForm::Flag => "flag",
        SetForm::Passive => "passive",
        SetForm::Chained => "chained",
    }
}

fn op_name(op: &MutationOp) -> &'static str {
    match op {
        MutationOp::Append => "append",
        MutationOp::Prepend => "prepend",
        MutationOp::Extend => "extend",
        MutationOp::Insert => "insert",
        MutationOp::Remove => "remove",
        MutationOp::Replace => "replace",
        MutationOp::Empty => "empty",
        MutationOp::Clear => "clear",
        MutationOp::MapSet => "map_set",
        MutationOp::MapRemove => "map_remove",
        MutationOp::Enqueue => "enqueue",
        MutationOp::Dequeue => "dequeue",
        MutationOp::Increment => "increment",
        MutationOp::Decrement => "decrement",
    }
}

fn init_form_name(form: InitForm) -> &'static str {
    match form {
        InitForm::WhoseList => "whose_list",
        InitForm::WithItsSetTo => "with_its_set_to",
        InitForm::WithList => "with_list",
        InitForm::DlEntries => "dl_entries",
    }
}

fn reason_name(reason: OpaqueReason) -> &'static str {
    match reason {
        OpaqueReason::UnparsedTarget => "unparsed_target",
        OpaqueReason::PronounRoot => "pronoun_root",
        OpaqueReason::ValueIsInvocation => "value_is_invocation",
        OpaqueReason::UnsupportedForm => "unsupported_form",
        OpaqueReason::Other => "other",
    }
}

pub(crate) fn class_name(class: OccurrenceClass) -> &'static str {
    match class {
        OccurrenceClass::Write => "write",
        OccurrenceClass::Init => "init",
        OccurrenceClass::ReadPath => "read_path",
        OccurrenceClass::Read => "read",
        OccurrenceClass::Unclassified => "unclassified",
    }
}

pub(crate) fn role_name(role: ProseRole) -> &'static str {
    match role {
        ProseRole::Steps => "steps",
        ProseRole::Getter => "getter",
        ProseRole::Setter => "setter",
        ProseRole::Method => "method",
        ProseRole::Constructor => "constructor",
        ProseRole::Normative => "normative",
    }
}

fn anchor_key(target: &AnchorTarget) -> String {
    format!("{}#{}", target.spec, target.anchor)
}

fn root_name(root: &Root) -> &'static str {
    match root {
        Root::Var(_) => "var",
        Root::This => "this",
        Root::Link { .. } => "link",
        Root::Implicit => "implicit",
        Root::Opaque { .. } => "opaque",
    }
}

/// `var`, `this`, `link` (each with `_subscript`), `field:<root>`,
/// `field_subscript:<root>`, `code_member`, `slot` or `other`.
fn path_shape(path: &Path) -> String {
    let root = root_name(&path.root);
    let subscript = if path.subscript.is_some() {
        "_subscript"
    } else {
        ""
    };
    match path.hops.last() {
        None if matches!(path.root, Root::Var(_) | Root::This | Root::Link { .. }) => {
            format!("{root}{subscript}")
        }
        None => "other".to_string(),
        Some(Hop::Field { .. }) => format!("field{subscript}:{root}"),
        Some(Hop::CodeMember { .. }) => "code_member".to_string(),
        Some(Hop::Slot { .. }) => "slot".to_string(),
    }
}

/// The serde tag of a statement kind.
fn kind_tag(kind: &StatementKind) -> &'static str {
    match kind {
        StatementKind::Let { .. } => "let",
        StatementKind::Set { .. } => "set",
        StatementKind::Mutate { .. } => "mutate",
        StatementKind::Init { .. } => "init",
        StatementKind::Opaque { .. } => "opaque",
        StatementKind::Call { .. } => "call",
        StatementKind::If { .. } => "if",
        StatementKind::Otherwise { .. } => "otherwise",
        StatementKind::ForEach { .. } => "for_each",
        StatementKind::While { .. } => "while",
        StatementKind::Return { .. } => "return",
        StatementKind::Throw { .. } => "throw",
        StatementKind::Abort { .. } => "abort",
        StatementKind::Continue => "continue",
        StatementKind::Break => "break",
        StatementKind::ContinueRemaining { .. } => "continue_remaining",
        StatementKind::Wait { .. } => "wait",
        StatementKind::InParallel { .. } => "in_parallel",
        StatementKind::RunSteps { .. } => "run_steps",
        StatementKind::Assert { .. } => "assert",
    }
}

/// Statement-form key: `set:<form>:<shape>`, `mutate:<op>:<shape>`,
/// `init:<form>`, `opaque:<reason>:<verb or none>`; the kind tag otherwise.
fn statement_key(kind: &StatementKind) -> String {
    match kind {
        StatementKind::Set { targets, form, .. } => {
            let shape = targets.first().map_or("other".to_string(), path_shape);
            format!("set:{}:{shape}", set_form_name(*form))
        }
        StatementKind::Mutate { op, target, .. } => {
            format!("mutate:{}:{}", op_name(op), path_shape(target))
        }
        StatementKind::Init { form, .. } => format!("init:{}", init_form_name(*form)),
        StatementKind::Opaque { reason, verb, .. } => {
            format!(
                "opaque:{}:{}",
                reason_name(*reason),
                verb.as_deref().unwrap_or("none")
            )
        }
        other => kind_tag(other).to_string(),
    }
}

/// Statement forms, and structured and unstructured `Set`s: a `Set … to be
/// ⟦L⟧` call counts as an unstructured set, as the opaque `set` it replaced.
fn count_statements(statements: &[Statement], index: &IrIndex, coverage: &mut CoverageCounters) {
    for statement in statements {
        *coverage
            .statements
            .entry(statement_key(&statement.kind))
            .or_default() += 1;
        match &statement.kind {
            StatementKind::Set { .. }
            | StatementKind::Mutate {
                op: MutationOp::MapSet,
                ..
            } => {
                coverage.set_total += 1;
                coverage.set_structured += 1;
            }
            StatementKind::Opaque {
                verb: Some(verb), ..
            } if verb == "set" || verb == "unset" => coverage.set_total += 1,
            StatementKind::Call { call }
                if index
                    .calls
                    .get(call.as_str())
                    .is_some_and(|call| call.form == CallForm::SetToBe) =>
            {
                coverage.set_total += 1
            }
            _ => {}
        }
    }
}

fn step_path(context: &SourceContext) -> Option<&str> {
    match context {
        SourceContext::Algorithm { step_path, .. } | SourceContext::Prose { step_path, .. } => {
            step_path.as_deref()
        }
        SourceContext::BranchLabel { step_path, .. } => Some(step_path),
        SourceContext::Intro { .. } => None,
    }
}

fn step_id(context: &SourceContext) -> Option<&str> {
    match context {
        SourceContext::Algorithm { step_id, .. } => step_id.as_deref(),
        SourceContext::BranchLabel { step_id, .. } => Some(step_id),
        SourceContext::Prose { .. } | SourceContext::Intro { .. } => None,
    }
}

/// Occurrence classes over local owned fields, written fields by owner basis,
/// unresolved links and the unclassified review list (§5.1, §5.3). A written
/// field is a local `FieldDef` or concept dfn that is the target of a `Write`.
fn count_occurrences(
    spec: &str,
    fields: &[FieldDef],
    concept_ids: &HashSet<&str>,
    sources: &[StatementSource],
    occurrences: &[Occurrence],
    coverage: &mut CoverageCounters,
) {
    let fields: HashMap<&str, &FieldDef> = fields
        .iter()
        .filter(|field| field.anchor.spec == spec)
        .map(|field| (field.anchor.anchor.as_str(), field))
        .collect();
    let sources_by_id: HashMap<&str, &StatementSource> =
        sources.iter().map(|s| (s.id.as_str(), s)).collect();
    coverage.unresolved_links = sources
        .iter()
        .filter(|source| !matches!(source.context, SourceContext::Intro { .. }))
        .flat_map(|source| &source.links)
        .filter(|link| link.target.is_none())
        .count() as u32;

    let mut written = BTreeSet::new();
    for occurrence in occurrences {
        let local = occurrence
            .target
            .as_ref()
            .filter(|target| target.spec == spec)
            .map(|target| target.anchor.as_str());
        if let Some(anchor) = local {
            let owned = fields
                .get(anchor)
                .is_some_and(|field| matches!(field.owner, Owner::Known { .. }));
            if owned {
                *coverage
                    .occurrences
                    .entry(class_name(occurrence.class).to_string())
                    .or_default() += 1;
            }
            let is_field = fields.contains_key(anchor) || concept_ids.contains(anchor);
            if occurrence.class == OccurrenceClass::Write && is_field {
                written.insert(anchor);
            }
        }
        if occurrence.class == OccurrenceClass::Unclassified {
            let source = sources_by_id.get(occurrence.source_id.as_str());
            coverage.unclassified_review.push(ReviewItem {
                subject: source.map_or_else(String::new, |s| anchor_key(&s.subject)),
                step_path: source
                    .and_then(|s| step_path(&s.context))
                    .map(str::to_string),
                target: occurrence
                    .target
                    .as_ref()
                    .map_or_else(|| "unresolved".to_string(), anchor_key),
                text: source.map_or_else(String::new, |s| s.text.clone()),
            });
        }
    }

    coverage.written_fields = written.len() as u32;
    for anchor in written {
        match fields.get(anchor).map(|field| &field.owner) {
            Some(Owner::Known { basis, .. }) => {
                *coverage
                    .written_fields_owned
                    .entry(declare::rule_key(basis).to_string())
                    .or_default() += 1;
            }
            _ => coverage.written_fields_unresolved.push(anchor.to_string()),
        }
    }
}

/// `site-` + sha256(source_id \0 link_id-or-statement_id \0 class).
pub(crate) fn site_id(source_id: &str, local_id: &str, class: SiteClass) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_id.as_bytes());
    for component in [local_id, class.as_str()] {
        hasher.update([0]);
        hasher.update(component.as_bytes());
    }
    format!("site-{:x}", hasher.finalize())
}

/// The parts of a site that depend on its class and statement.
pub(crate) struct SiteParts {
    pub op: String,
    pub receiver: &'static str,
    pub constructed: Option<String>,
    pub target_text: String,
    pub value_text: Option<String>,
}

/// One `state_sites` row per `Write`/`Init`/`Unclassified` occurrence with a
/// target, then one `OpaqueWrite` row per `Opaque` statement with a verb (§9.2).
pub fn derive_sites(state: &StateSpec) -> Vec<Site> {
    let sources: HashMap<&str, &StatementSource> =
        state.sources.iter().map(|s| (s.id.as_str(), s)).collect();
    let statements: HashMap<&str, &Statement> = state
        .statements
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect();
    let calls: HashMap<&str, &Call> = state.calls.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut first_clause: HashMap<&str, usize> = HashMap::new();
    let mut opaque_by_source: HashMap<&str, Vec<&Statement>> = HashMap::new();
    let mut rule_clauses: HashSet<(&str, usize)> = HashSet::new();
    for statement in &state.statements {
        first_clause
            .entry(statement.source_id.as_str())
            .and_modify(|start| *start = (*start).min(statement.span.start))
            .or_insert(statement.span.start);
        match statement.kind {
            StatementKind::Opaque { verb: Some(_), .. } => opaque_by_source
                .entry(statement.source_id.as_str())
                .or_default()
                .push(statement),
            StatementKind::Mutate {
                basis: OpBasis::Rule { .. },
                ..
            } => {
                rule_clauses.insert((statement.source_id.as_str(), statement.span.start));
            }
            _ => {}
        }
    }
    let type_anchors: HashMap<&AnchorTarget, &TypeKey> = state
        .model
        .types
        .iter()
        .flat_map(|ty| ty.anchors.iter().map(move |a| (&a.target, &ty.key)))
        .collect();

    let mut sites = Vec::new();
    for occurrence in &state.occurrences {
        let class = match occurrence.class {
            OccurrenceClass::Write => SiteClass::Write,
            OccurrenceClass::Init => SiteClass::Init,
            OccurrenceClass::Unclassified => SiteClass::Unclassified,
            OccurrenceClass::Read | OccurrenceClass::ReadPath => continue,
        };
        let Some(target) = &occurrence.target else {
            continue;
        };
        let Some(source) = sources.get(occurrence.source_id.as_str()) else {
            continue;
        };
        let Some(link) = source.links.iter().find(|l| l.id == occurrence.link_id) else {
            continue;
        };
        let statement = occurrence
            .statement_id
            .as_deref()
            .and_then(|id| statements.get(id).copied());
        let parts = match class {
            // A write rule reclassified an unclassified link: its clause's verb.
            SiteClass::Write if statement.is_none() => SiteParts {
                op: clause_verb(&opaque_by_source, source, link),
                ..write_parts(source, None, link)
            },
            SiteClass::Write => write_parts(source, statement, link),
            SiteClass::Init => init_parts(source, statement, link, &type_anchors),
            _ => SiteParts {
                op: clause_verb(&opaque_by_source, source, link),
                receiver: "opaque",
                constructed: None,
                target_text: link_text(source, link),
                value_text: None,
            },
        };
        let mut row = site(
            source,
            site_text(source, &first_clause),
            site_id(&source.id, &link.id, class),
            class,
            Some(target.clone()),
            parts,
            &occurrence.basis,
            statement.as_ref().map(|s| s.span),
        );
        if let SourceContext::BranchLabel {
            segment_id,
            value_segment_id,
            ..
        } = &source.context
        {
            if let Some(intro) = sources.get(segment_id.as_str()) {
                row.text = intro.text.clone();
            }
            row.target_text = source.text.clone();
            row.value_text = value_segment_id
                .as_deref()
                .and_then(|id| sources.get(id))
                .and_then(|value| text(value, 0, value.text.len()));
        }
        sites.push(row);
    }

    for statement in &state.statements {
        if rule_clauses.contains(&(statement.source_id.as_str(), statement.span.start)) {
            continue;
        }
        let Some(source) = sources.get(statement.source_id.as_str()) else {
            continue;
        };
        let (op, target_text) = match &statement.kind {
            StatementKind::Opaque {
                verb: Some(verb),
                target_text,
                ..
            } => (verb.clone(), target_text.clone().unwrap_or_default()),
            // SP4 replaces these with the callee's propagated writes.
            StatementKind::Call { call }
                if calls
                    .get(call.as_str())
                    .is_some_and(|call| call.form == CallForm::SetToBe) =>
            {
                ("set".to_string(), set_to_be_target(source, statement.span))
            }
            _ => continue,
        };
        let class = SiteClass::OpaqueWrite;
        let parts = SiteParts {
            op,
            receiver: "opaque",
            constructed: None,
            target_text,
            value_text: None,
        };
        sites.push(site(
            source,
            site_text(source, &first_clause),
            site_id(&source.id, &statement.id, class),
            class,
            None,
            parts,
            "ir",
            Some(statement.span),
        ));
    }
    sites.extend(state.declared_sites.iter().cloned());
    sites.extend(state.model.reflections.iter().filter_map(reflection_site));
    sites
}

/// The text of a `Set … to be ⟦L⟧` statement between the `Set` keyword and
/// the first ` to ` after it.
fn set_to_be_target(source: &StatementSource, span: TextSpan) -> String {
    let Some(clause) = source.text.get(span.start..span.end) else {
        return String::new();
    };
    let target = ["Set ", "set "]
        .iter()
        .filter_map(|keyword| clause.find(keyword).map(|at| at + keyword.len()))
        .min()
        .map_or(clause, |after| &clause[after..]);
    target
        .find(" to ")
        .map_or(target, |to| &target[..to])
        .trim()
        .to_string()
}

/// The `Declared` reflect site of a reflection whose content attribute is known.
fn reflection_site(reflection: &Reflection) -> Option<Site> {
    let target = reflection.content_attribute.clone()?;
    let subject = &reflection.idl_attribute;
    Some(Site {
        site_id: site_id(
            &format!("{}#{}", subject.spec, subject.anchor),
            &format!("{}#{}", target.spec, target.anchor),
            SiteClass::Declared,
        ),
        class: SiteClass::Declared,
        target: Some(target),
        op: "reflect".to_string(),
        subject: subject.clone(),
        context: "reflection".to_string(),
        role: Some(reflect::basis_label(&reflection.basis)),
        constructed: None,
        step_path: None,
        step_id: None,
        receiver: "this".to_string(),
        target_text: reflection.content_attribute_name.clone(),
        value_text: None,
        text: format!(
            "The {} IDL attribute reflects the {} content attribute.",
            reflection.idl_attribute_name, reflection.content_attribute_name
        ),
        basis: "reflect".to_string(),
        segment_id: None,
        body_id: None,
        span_start: None,
        span_end: None,
    })
}

/// A prose source's text from the start of its first statement's clause to
/// the end of that sentence, `… `-prefixed when that is not the start; any
/// other source's whole text.
fn site_text(source: &StatementSource, first_clause: &HashMap<&str, usize>) -> String {
    let (SourceContext::Prose { .. }, Some(&start)) =
        (&source.context, first_clause.get(source.id.as_str()))
    else {
        return source.text.clone();
    };
    let rest = &source.text[start..];
    let sentence = rest.find(". ").map_or(rest, |end| &rest[..=end]);
    if start == 0 {
        sentence.to_string()
    } else {
        format!("… {sentence}")
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn site(
    source: &StatementSource,
    text: String,
    site_id: String,
    class: SiteClass,
    target: Option<AnchorTarget>,
    parts: SiteParts,
    basis: &str,
    statement_span: Option<TextSpan>,
) -> Site {
    let (context, role) = match &source.context {
        SourceContext::Algorithm { .. } => ("algorithm", None),
        SourceContext::BranchLabel { .. } => ("branch_label", None),
        SourceContext::Prose { role, .. } => ("prose", Some(role_name(*role).to_string())),
        SourceContext::Intro { .. } => ("intro", None),
    };
    let (segment_id, body_id) = match &source.context {
        SourceContext::Algorithm {
            segment_id,
            body_id,
            ..
        } => (Some(segment_id.clone()), Some(body_id.clone())),
        SourceContext::BranchLabel { segment_id, .. } => (Some(segment_id.clone()), None),
        SourceContext::Prose { .. } | SourceContext::Intro { .. } => (None, None),
    };
    let (span_start, span_end) = match statement_span {
        Some(span) => (
            Some(span.start.min(u32::MAX as usize) as u32),
            Some(span.end.min(u32::MAX as usize) as u32),
        ),
        None => (None, None),
    };
    Site {
        site_id,
        class,
        target,
        op: parts.op,
        subject: source.subject.clone(),
        context: context.to_string(),
        role,
        constructed: parts.constructed,
        step_path: step_path(&source.context).map(str::to_string),
        step_id: step_id(&source.context).map(str::to_string),
        receiver: parts.receiver.to_string(),
        target_text: parts.target_text,
        value_text: parts.value_text,
        text,
        basis: basis.to_string(),
        segment_id,
        body_id,
        span_start,
        span_end,
    }
}

fn link_text(source: &StatementSource, link: &LinkSpan) -> String {
    source.text[link.span.start..link.span.end].to_string()
}

/// Trimmed source text of `start..end`, if not empty.
fn text(source: &StatementSource, start: usize, end: usize) -> Option<String> {
    let slice = source.text.get(start..end)?.trim();
    (!slice.is_empty()).then(|| slice.to_string())
}

/// The verb of the unconsumed clause a link sits in: that of the `Opaque`
/// statement covering it.
fn clause_verb(
    opaque_by_source: &HashMap<&str, Vec<&Statement>>,
    source: &StatementSource,
    link: &LinkSpan,
) -> String {
    opaque_by_source
        .get(source.id.as_str())
        .into_iter()
        .flatten()
        .find(|st| st.span.start <= link.span.start && link.span.start < st.span.end)
        .and_then(|st| match &st.kind {
            StatementKind::Opaque { verb, .. } => verb.clone(),
            _ => None,
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// The last occurrence of `word` in `haystack` that is not part of a longer word.
fn rfind_word(haystack: &str, word: &str) -> Option<usize> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    haystack.rmatch_indices(word).map(|(at, _)| at).find(|&at| {
        !haystack[..at].chars().next_back().is_some_and(is_word)
            && !haystack[at + word.len()..]
                .chars()
                .next()
                .is_some_and(is_word)
    })
}

fn last_hop_is(path: &Path, link_id: &str) -> bool {
    matches!(path.hops.last(), Some(Hop::Field { link_id: id, .. }) if id == link_id)
}

fn link_span_start(source: &StatementSource, link_id: &str) -> Option<usize> {
    source
        .links
        .iter()
        .find(|link| link.id == link_id)
        .map(|link| link.span.start)
}

/// Where the text of `path` starts in the statement starting at `from`: the
/// earliest of its root and field hops, with a leading `the `.
fn path_start(source: &StatementSource, path: &Path, from: usize, write: &LinkSpan) -> usize {
    let first_hop = path
        .hops
        .iter()
        .filter_map(|hop| match hop {
            Hop::Field { link_id, .. } => link_span_start(source, link_id),
            _ => None,
        })
        .filter(|&start| start >= from)
        .min()
        .unwrap_or(write.span.start)
        .max(from);
    let before = &source.text[from..first_hop];
    let root = match &path.root {
        Root::Var(name) => source
            .tokens
            .iter()
            .filter(|t| t.kind == InlineTokenKind::Variable && t.source_text == *name)
            .filter(|t| t.span.start >= from && t.span.end <= first_hop)
            .map(|t| t.span.start)
            .next_back(),
        Root::Link { link_id, .. } => link_span_start(source, link_id),
        Root::This => rfind_word(before, "this").map(|at| from + at),
        Root::Opaque { text } => rfind_word(before, text).map(|at| from + at),
        Root::Implicit => None,
    };
    let start = root.map_or(first_hop, |root| root.min(first_hop)).max(from);
    let lead = &source.text[from..start];
    if lead.ends_with("the ") || lead.ends_with("The ") {
        start - "the ".len()
    } else {
        start
    }
}

/// `Set`/`Mutate` write: op, receiver, target text and value text.
fn write_parts(
    source: &StatementSource,
    statement: Option<&Statement>,
    link: &LinkSpan,
) -> SiteParts {
    let fallback = || SiteParts {
        op: "unknown".to_string(),
        receiver: "opaque",
        constructed: None,
        target_text: link_text(source, link),
        value_text: None,
    };
    let Some(statement) = statement else {
        return fallback();
    };
    let span = statement.span;
    let (op, path, separator, operand_before) = match &statement.kind {
        StatementKind::Set {
            targets,
            value,
            form,
        } => {
            let path = targets
                .iter()
                .find(|path| last_hop_is(path, &link.id))
                .or(targets.first());
            let op = match (form, value) {
                (SetForm::Flag, Expr::Literal(Literal::Bool(false))) => "unset",
                _ => "set",
            };
            let separator = match form {
                SetForm::Flag => None,
                SetForm::Passive => Some(" must be set to "),
                SetForm::To | SetForm::Chained => Some(" to "),
            };
            (op.to_string(), path, separator, false)
        }
        StatementKind::Mutate {
            op,
            target,
            operand,
            ..
        } => {
            let separator = match op {
                MutationOp::MapSet => Some(" to "),
                MutationOp::Increment | MutationOp::Decrement => operand.as_ref().map(|_| " by "),
                _ => None,
            };
            let before = separator.is_none() && operand.is_some();
            (op_name(op).to_string(), Some(target), separator, before)
        }
        _ => return fallback(),
    };
    let Some(path) = path else {
        return fallback();
    };
    let start = path_start(source, path, span.start, link);
    let (target_text, value_text) = match separator {
        Some(separator) => {
            let found = source
                .text
                .get(link.span.end..span.end)
                .and_then(|rest| rest.find(separator))
                .map(|at| link.span.end + at);
            match found {
                Some(at) => (
                    text(source, start, at),
                    text(source, at + separator.len(), span.end),
                ),
                None => (text(source, start, span.end), None),
            }
        }
        None if operand_before => (
            text(source, start, link.span.end),
            operand_text(source, span.start, start),
        ),
        None => (text(source, start, span.end.max(link.span.end)), None),
    };
    SiteParts {
        op,
        receiver: root_name(&path.root),
        constructed: None,
        target_text: target_text.unwrap_or_else(|| link_text(source, link)),
        value_text,
    }
}

/// The operand of `VERB OPERAND PREP TARGET` (Infra mutations): the text
/// between the verb and the target, without the preposition.
fn operand_text(source: &StatementSource, from: usize, target_start: usize) -> Option<String> {
    let head = source.text.get(from..target_start)?;
    let verb_end = source
        .links
        .iter()
        .find(|link| link.span.start == from)
        .map(|link| link.span.end)
        .or_else(|| head.find(' ').map(|at| from + at))?;
    let operand = source.text.get(verb_end..target_start)?.trim_end();
    let operand = [" into", " from", " to", " in", " on"]
        .iter()
        .find_map(|prep| operand.strip_suffix(prep))
        .unwrap_or(operand)
        .trim();
    (!operand.is_empty()).then(|| operand.to_string())
}

/// `Init` entry: the field hop text, the entry value, and the constructed type
/// canonicalized through the local type anchors.
fn init_parts(
    source: &StatementSource,
    statement: Option<&Statement>,
    link: &LinkSpan,
    type_anchors: &HashMap<&AnchorTarget, &TypeKey>,
) -> SiteParts {
    let mut parts = SiteParts {
        op: "init".to_string(),
        receiver: "new",
        constructed: None,
        target_text: link_text(source, link),
        value_text: None,
    };
    let Some(StatementKind::Init {
        constructed,
        entries,
        ..
    }) = statement.map(|s| &s.kind)
    else {
        return parts;
    };
    parts.constructed = constructed.as_ref().map(|ty| match ty {
        TypeRef::Known(key) => key.to_string(),
        TypeRef::Unresolved(target) => type_anchors
            .get(target)
            .map_or_else(|| anchor_key(target), |key| key.to_string()),
    });
    let index = entries.iter().position(
        |entry| matches!(&entry.field, Hop::Field { link_id, .. } if *link_id == link.id),
    );
    if let (Some(index), Some(statement)) = (index, statement) {
        let next_start = entries.get(index + 1).and_then(|entry| match &entry.field {
            Hop::Field { link_id, .. } => link_span_start(source, link_id),
            _ => None,
        });
        parts.value_text = entry_value_text(source, link.span.end, next_start, statement.span.end);
    }
    parts
}

/// `⟦F⟧ is VALUE` / `⟦F⟧ set to VALUE`, up to the next entry or the statement end.
fn entry_value_text(
    source: &StatementSource,
    field_end: usize,
    next_start: Option<usize>,
    statement_end: usize,
) -> Option<String> {
    let rest = source.text.get(field_end..)?;
    let value_start = field_end
        + [" is ", " set to "]
            .iter()
            .find(|word| rest.starts_with(*word))
            .map(|word| word.len())?;
    let value = source
        .text
        .get(value_start..next_start.unwrap_or(statement_end))?
        .trim_end();
    let value = value.strip_suffix(" its").unwrap_or(value);
    let value = [", and", " and", ","]
        .iter()
        .find_map(|separator| value.strip_suffix(separator))
        .unwrap_or(value)
        .trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Per target: totals of `read` and `read_path` occurrences and prose mentions.
pub fn derive_occurrence_counts(state: &StateSpec) -> Vec<(AnchorTarget, &'static str, u32)> {
    let mut counts: BTreeMap<(AnchorTarget, &'static str), u32> = BTreeMap::new();
    for occurrence in &state.occurrences {
        let class = match occurrence.class {
            OccurrenceClass::Read => "read",
            OccurrenceClass::ReadPath => "read_path",
            _ => continue,
        };
        if let Some(target) = &occurrence.target {
            *counts.entry((target.clone(), class)).or_default() += 1;
        }
    }
    for (key, count) in &state.prose_mentions {
        if let Some((spec, anchor)) = key.split_once('#') {
            let target = AnchorTarget {
                spec: spec.to_string(),
                anchor: anchor.to_string(),
            };
            *counts.entry((target, "prose_mention")).or_default() += count;
        }
    }
    counts
        .into_iter()
        .map(|((target, class), count)| (target, class, count))
        .collect()
}

/// Source index of each `<dl>` initializer entry → the constructed type and
/// the entry value.
pub(crate) type BranchInits = HashMap<usize, (Option<TypeRef>, Expr)>;

/// One source per structural segment, in algorithm and segment order, then
/// per algorithm one source per `<dl>` initializer entry (§7.1).
pub(crate) fn algorithm_sources(structure: &StructuralSpec) -> (Vec<StatementSource>, BranchInits) {
    let mut sources = Vec::new();
    let mut inits = BranchInits::new();
    for algorithm in &structure.algorithms {
        let subject = AnchorTarget {
            spec: structure.spec.clone(),
            anchor: algorithm.source.section_anchor.clone(),
        };
        let step_paths: BTreeMap<&str, String> = algorithm
            .steps
            .iter()
            .map(|step| {
                let path = step
                    .path
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(".");
                (step.source.node_id.as_str(), path)
            })
            .collect();
        let mut segment_sources: HashMap<&str, usize> = HashMap::new();
        for segment in &algorithm.segments {
            let step_path = segment
                .owner_step_id
                .as_deref()
                .and_then(|id| step_paths.get(id).cloned());
            segment_sources.insert(segment.source.node_id.as_str(), sources.len());
            sources.push(StatementSource {
                id: segment.source.node_id.clone(),
                subject: subject.clone(),
                context: SourceContext::Algorithm {
                    segment_id: segment.source.node_id.clone(),
                    step_id: segment.owner_step_id.clone(),
                    step_path,
                    body_id: segment.owner_body_id.clone(),
                },
                text: segment.text.clone(),
                tokens: segment.tokens.clone(),
                links: segment.links.clone(),
            });
        }

        let branches: HashMap<&str, &StructuralBranch> = algorithm
            .branches
            .iter()
            .map(|branch| (branch.source.node_id.as_str(), branch))
            .collect();
        for step in &algorithm.steps {
            let Some(first_branch) = step
                .items
                .iter()
                .position(|item| matches!(item, StepItem::Branch(_)))
            else {
                continue;
            };
            let entries: Vec<&StructuralBranch> = step.items[first_branch..]
                .iter()
                .filter_map(|item| match item {
                    StepItem::Branch(id) => branches.get(id.as_str()).copied(),
                    _ => None,
                })
                .filter(|branch| !branch.label_links.is_empty())
                .collect();
            let intro = step.items[..first_branch]
                .iter()
                .rev()
                .find_map(|item| match item {
                    StepItem::Segment(id) => segment_sources.get_key_value(id.as_str()),
                    _ => None,
                });
            let (Some((&intro_id, &intro_index)), false) = (intro, entries.is_empty()) else {
                continue;
            };
            let Some(constructed) = ir::initializer_intro(&sources[intro_index]) else {
                continue;
            };
            let step_path = step_paths
                .get(step.source.node_id.as_str())
                .cloned()
                .unwrap_or_default();
            for branch in entries {
                let value = branch.items.iter().find_map(|item| match item {
                    StepItem::Segment(id) => segment_sources.get_key_value(id.as_str()),
                    _ => None,
                });
                let value_expr = value.map_or_else(
                    || Expr::Opaque {
                        text: String::new(),
                    },
                    |(_, &index)| ir::parse_value(&sources[index]),
                );
                inits.insert(sources.len(), (constructed.clone(), value_expr));
                sources.push(StatementSource {
                    id: branch.source.node_id.clone(),
                    subject: subject.clone(),
                    context: SourceContext::BranchLabel {
                        branch_id: branch.source.node_id.clone(),
                        step_id: step.source.node_id.clone(),
                        step_path: step_path.clone(),
                        segment_id: intro_id.to_string(),
                        value_segment_id: value.map(|(&id, _)| id.to_string()),
                    },
                    text: branch.label_text.clone(),
                    tokens: branch.label_tokens.clone(),
                    links: branch.label_links.clone(),
                });
            }
        }
    }
    (sources, inits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::testing::{extract_html as extract, DL_HTML, MINI};
    use crate::state::{OwnerBasis, StateIssueCode, SuperBasis, STATE_VERSION};

    #[test]
    fn declarations_bind_names_add_edges_and_override_owners() {
        let dom = r##"<pre class="idl">interface <dfn data-dfn-type="interface" id="nodeiterator">NodeIterator</dfn> {}; interface <dfn data-dfn-type="interface" id="treewalker">TreeWalker</dfn> {};</pre>
      <h2 id="traversal">Traversal</h2>
      <p>Each <code><a href="#nodeiterator">NodeIterator</a></code> and <code><a href="#treewalker">TreeWalker</a></code> object has an associated boolean <dfn data-dfn-for="traversal" data-dfn-type="dfn" id="concept-traversal-active">is active</dfn> to avoid recursive invocations. It is initially false.</p>"##;
        let yaml = "schema: 1\npackage: p\ntypes:\n  - id: dom-traversal\n    type: DOM#traversal\n    name: traversal\n    kind: concept\n    implemented_by: [idl:NodeIterator, idl:TreeWalker]\n    expect_text: '(?i)Each NodeIterator and TreeWalker object has an associated'\n    reason: r\n";
        let catalog = crate::state::catalog::load_state_files(&[("state/t.yaml", yaml)]).unwrap();
        let state = crate::state::testing::extract_with_catalog(dom, "DOM", &catalog);
        let f = state
            .model
            .fields
            .iter()
            .find(|f| f.anchor.anchor == "concept-traversal-active")
            .unwrap();
        assert!(
            matches!(&f.owner, Owner::Known { types, basis: OwnerBasis::DfnFor } if types[0].key.to_string() == "DOM#traversal")
        );
        let iterator = state
            .model
            .types
            .iter()
            .find(|t| t.key.to_string() == "idl:NodeIterator")
            .unwrap();
        assert!(iterator
            .supertypes
            .iter()
            .any(|e| e.target.to_string() == "DOM#traversal"
                && matches!(e.basis, SuperBasis::Override { .. })));
        assert!(state
            .representation_version
            .starts_with(&format!("{STATE_VERSION}+sha256:")));
    }

    #[test]
    fn mismatched_expect_text_is_reported_and_ignored() {
        let html = r#"<p>Some video files also have an explicit date, known as the <dfn id="timeline-offset">timeline offset</dfn>.</p>"#;
        let yaml = "schema: 1\npackage: p\nfields:\n  - id: t\n    field: HTML#timeline-offset\n    owner: [HTML#media-resource]\n    expect_text: 'does not occur'\n    reason: r\n";
        let catalog = crate::state::catalog::load_state_files(&[("state/f.yaml", yaml)]).unwrap();
        let state = crate::state::testing::extract_with_catalog(html, "HTML", &catalog);
        assert!(state
            .issues
            .iter()
            .any(|i| i.code == StateIssueCode::DeclarationMismatch));
        assert!(state.model.fields.iter().all(|f| !matches!(
            f.owner,
            Owner::Known {
                basis: OwnerBasis::Override { .. },
                ..
            }
        )));
    }

    #[test]
    fn matching_field_declaration_creates_the_field_and_overrides_its_owner() {
        let html = r#"<p>Some video files also have an explicit date, known as the <dfn id="timeline-offset">timeline offset</dfn>.</p>"#;
        let yaml = "schema: 1\npackage: p\nfields:\n  - id: t\n    field: HTML#timeline-offset\n    owner: [HTML#media-resource]\n    expect_text: 'known as the timeline offset'\n    reason: r\n";
        let catalog = crate::state::catalog::load_state_files(&[("state/f.yaml", yaml)]).unwrap();
        let state = crate::state::testing::extract_with_catalog(html, "HTML", &catalog);
        let f = &state.model.fields[0];
        assert!(
            matches!(&f.owner, Owner::Known { types, basis: OwnerBasis::Override { rule_id } }
                if rule_id == "p/t" && types[0].key.to_string() == "HTML#media-resource")
        );
        assert_eq!(state.coverage.owner_by_rule.get("override"), Some(&1));
        assert!(state.issues.is_empty());
    }

    #[test]
    fn heading_declaration_without_owned_matching_field_is_withdrawn() {
        let html = r#"<h2 id="traversal">Traversal</h2>
      <p>Each thing has an associated <dfn data-dfn-for="traversal" id="active">is active</dfn>.</p>"#;
        let yaml = "schema: 1\npackage: p\ntypes:\n  - id: t\n    type: DOM#traversal\n    name: traversal\n    expect_text: 'does not occur'\n    reason: r\n";
        let catalog = crate::state::catalog::load_state_files(&[("state/t.yaml", yaml)]).unwrap();
        let state = crate::state::testing::extract_with_catalog(html, "DOM", &catalog);
        assert!(state
            .issues
            .iter()
            .any(|i| i.code == StateIssueCode::DeclarationMismatch
                && i.anchor.as_deref() == Some("traversal")));
        assert!(matches!(state.model.fields[0].owner, Owner::Unknown { .. }));
        assert!(state
            .model
            .types
            .iter()
            .all(|t| t.key.to_string() != "DOM#traversal"));
    }

    #[test]
    fn mini_spec_produces_field_sites_and_counts() {
        let state = extract(MINI, "HTML");
        assert_eq!(state.representation_version, STATE_VERSION);
        assert_eq!(state.model.fields.len(), 1);
        let sites = derive_sites(&state);
        let writes: Vec<_> = sites
            .iter()
            .filter(|s| s.class == SiteClass::Write)
            .collect();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].step_path.as_deref(), Some("1"));
        assert_eq!(writes[0].receiver, "var");
        assert_eq!(writes[0].value_text.as_deref(), Some("false"));
        assert_eq!(writes[0].subject.anchor, "document-open-steps");
        assert!(sites.iter().any(|s| s.class == SiteClass::Unclassified
            && s.step_path.as_deref() == Some("3")
            && s.op == "add"));
        assert!(sites
            .iter()
            .any(|s| s.class == SiteClass::OpaqueWrite && s.op == "add"));
        let counts = derive_occurrence_counts(&state);
        assert!(counts
            .iter()
            .any(|(t, class, n)| t.anchor == "is-initial-about:blank"
                && *class == "read"
                && *n == 1));
        assert_eq!(state.coverage.written_fields, 1);
        assert_eq!(state.coverage.unclassified_review.len(), 1);
    }

    #[test]
    fn mini_spec_coverage_counters() {
        let coverage = extract(MINI, "HTML").coverage;
        assert_eq!(coverage.statements.get("set:to:field:var"), Some(&1));
        assert_eq!(
            coverage.statements.get("opaque:unsupported_form:add"),
            Some(&1)
        );
        assert_eq!((coverage.set_total, coverage.set_structured), (1, 1));
        let occurrences: Vec<_> = coverage
            .occurrences
            .iter()
            .map(|(class, n)| (class.as_str(), *n))
            .collect();
        assert_eq!(
            occurrences,
            [("read", 1), ("unclassified", 1), ("write", 1)]
        );
        assert_eq!(
            coverage.written_fields_owned.get("declaration_sentence"),
            Some(&1)
        );
        assert!(coverage.written_fields_unresolved.is_empty());
        let review = &coverage.unclassified_review[0];
        assert_eq!(
            (
                review.subject.as_str(),
                review.step_path.as_deref(),
                review.target.as_str()
            ),
            (
                "HTML#document-open-steps",
                Some("3"),
                "HTML#is-initial-about:blank"
            )
        );
    }

    #[test]
    fn subscripted_set_counts_as_a_structured_set() {
        let html = r##"<div data-algorithm=""><p>To <dfn id="run">run</dfn> given <var>x</var>:</p><ol>
<li><p>Set <var>x</var>[<var>k</var>] to v.</p></li></ol></div>"##;
        let coverage = extract(html, "HTML").coverage;
        assert_eq!(
            coverage.statements.get("mutate:map_set:var_subscript"),
            Some(&1)
        );
        assert_eq!((coverage.set_total, coverage.set_structured), (1, 1));
    }

    #[test]
    fn passive_and_pronoun_root_target_texts() {
        let html = r##"<div data-algorithm=""><p>To <dfn id="run">run</dfn> given <var>d</var>:</p><ol>
<li><p><var>d</var>'s <a href="#p">p</a> must be set to 1.</p></li>
<li><p>Set this item's <a href="#q">q</a> to 2, and set its <a href="#r">r</a> to 3.</p></li></ol></div>"##;
        let sites = derive_sites(&extract(html, "HTML"));
        let texts = |anchor: &str| {
            let site = sites
                .iter()
                .find(|s| s.target.as_ref().is_some_and(|t| t.anchor == anchor))
                .unwrap_or_else(|| panic!("no site for {anchor}: {sites:#?}"));
            (site.target_text.clone(), site.value_text.clone())
        };
        assert_eq!(texts("p"), ("*d*'s p".to_string(), Some("1".to_string())));
        assert_eq!(texts("r"), ("its r".to_string(), Some("3".to_string())));
    }

    #[test]
    fn site_parts_for_flags_mutations_and_initializers() {
        let html = r##"<div data-algorithm=""><p>To <dfn id="run">run</dfn> given <var>d</var>:</p><ol>
<li><p>Unset <var>d</var>'s <a href="#done-flag">done</a> flag.</p></li>
<li><p><a href="https://infra.spec.whatwg.org/#list-append">Append</a> <var>x</var> to <var>d</var>'s <a href="#pending">pending list</a>.</p></li>
<li><p>Return a new <a href="#concept-node">node</a>, with its <a href="#concept-node-document">node document</a> set to <var>d</var>.</p></li></ol></div>"##;
        let sites = derive_sites(&extract(html, "HTML"));
        let site = |anchor: &str| {
            sites
                .iter()
                .find(|s| s.target.as_ref().is_some_and(|t| t.anchor == anchor))
                .unwrap_or_else(|| panic!("no site for {anchor}: {sites:#?}"))
        };
        let flag = site("done-flag");
        assert_eq!(
            (
                flag.op.as_str(),
                flag.target_text.as_str(),
                flag.value_text.as_deref()
            ),
            ("unset", "*d*'s done flag", None)
        );
        let append = site("pending");
        assert_eq!(
            (
                append.op.as_str(),
                append.receiver.as_str(),
                append.target_text.as_str(),
                append.value_text.as_deref()
            ),
            ("append", "var", "*d*'s pending list", Some("*x*"))
        );
        let init = site("concept-node-document");
        assert_eq!(
            (
                init.class,
                init.op.as_str(),
                init.receiver.as_str(),
                init.target_text.as_str(),
                init.value_text.as_deref()
            ),
            (SiteClass::Init, "init", "new", "node document", Some("*d*"))
        );
        assert_eq!(init.constructed.as_deref(), Some("HTML#concept-node"));
        assert!(sites.iter().all(|s| s.site_id.starts_with("site-")));
    }

    #[test]
    fn dl_entries_are_initializations_with_label_context() {
        let state = extract(DL_HTML, "HTML");
        let sites = derive_sites(&state);
        let init = sites.iter().find(|s| s.class == SiteClass::Init).unwrap();
        assert_eq!(init.context, "branch_label");
        assert_eq!(init.step_path.as_deref(), Some("1"));
        assert_eq!(init.value_text.as_deref(), Some("true"));
        assert_eq!(init.constructed.as_deref(), Some("idl:Document"));
        assert_eq!(init.text, "Let *document* be a new `Document`, with:");
        assert_eq!(init.target_text, "is initial `about:blank`");
        assert!(state.statements.iter().any(|s| matches!(
            &s.kind,
            StatementKind::Init {
                form: InitForm::DlEntries,
                ..
            }
        )));
    }

    #[test]
    fn switch_dl_labels_are_not_initializers() {
        let html = r##"<div class="algorithm"><p>To <dfn id="a">a</dfn>:</p><ol><li><p>Switch on <var>x</var>:</p><dl class="switch"><dt><a href="#f">f</a></dt><dd>Return.</dd></dl></li></ol></div>"##;
        let state = extract(html, "HTML");
        assert!(!state
            .sources
            .iter()
            .any(|s| matches!(s.context, SourceContext::BranchLabel { .. })));
    }

    #[test]
    fn dl_entry_values_and_label_roles() {
        let html = r##"<div data-algorithm=""><p>To <dfn id="run">run</dfn>:</p><ol>
<li><p>Let <var>r</var> be a new <a href="#request">request</a> with</p>
<dl><dt><a href="#mode">mode</a> of <a href="#client">client</a></dt><dd>"<code>html</code>"</dd>
<dt><a href="#origin">origin</a></dt><dd><var>o</var></dd></dl></li></ol></div>"##;
        let state = extract(html, "HTML");
        let inits: Vec<_> = state
            .statements
            .iter()
            .filter_map(|s| match &s.kind {
                StatementKind::Init {
                    form: InitForm::DlEntries,
                    entries,
                    constructed,
                } => Some((entries, constructed)),
                _ => None,
            })
            .collect();
        assert_eq!(inits.len(), 2);
        assert_eq!(
            inits[0].0[0].value,
            Expr::EnumValue {
                text: "html".to_string(),
                target: None
            }
        );
        assert_eq!(inits[1].0[0].value, Expr::Var("o".to_string()));
        assert!(inits
            .iter()
            .all(|(_, c)| matches!(c, Some(TypeRef::Unresolved(t)) if t.anchor == "request")));
        let class_of = |anchor: &str| {
            state
                .occurrences
                .iter()
                .find(|o| o.target.as_ref().is_some_and(|t| t.anchor == anchor))
                .map(|o| o.class)
        };
        assert_eq!(class_of("mode"), Some(OccurrenceClass::Init));
        assert_eq!(class_of("client"), Some(OccurrenceClass::Read));
        assert_eq!(class_of("origin"), Some(OccurrenceClass::Init));
    }

    #[test]
    fn signatures_and_intro_sources_are_extracted_with_counters() {
        use crate::state::model::{SignatureForm, SignatureIssue, TemplatePiece as P};
        use crate::state::testing::{signature, NAV_HTML};
        let state = extract(NAV_HTML, "HTML");
        assert_eq!(signature(&state, "navigate").form, SignatureForm::To);
        let lon = signature(&state, "location-object-navigate");
        assert_eq!(
            lon.template.as_ref().unwrap().pieces,
            vec![
                P::Callee,
                P::Slot(0),
                P::Literal("to".into()),
                P::Slot(1),
                P::Literal("given".into()),
                P::Slot(2)
            ]
        );
        assert!(lon.params[2].optional);
        // `data-dfn-for="Location"` names the interface; NAV_HTML has no IDL block for it.
        let assign = signature(&state, "dom-location-assign");
        assert_eq!(
            assign.form,
            SignatureForm::IdlMethod {
                interface: "Location".into(),
                member: "assign".into()
            }
        );
        assert_eq!(assign.issues, vec![SignatureIssue::IdlMemberNotFound]);
        assert!(state
            .sources
            .iter()
            .any(|s| matches!(s.context, crate::state::ir::SourceContext::Intro { .. })));
        assert!(state
            .statements
            .iter()
            .all(|st| !st.source_id.starts_with("intro-")));
        let c = &state.coverage;
        assert_eq!(c.algorithms, 4);
        assert_eq!(
            (c.intro_forms.get("to"), c.intro_forms.get("idl_method")),
            (Some(&3), Some(&1))
        );
        assert_eq!(c.template_signatures, 3);
        assert_eq!(c.to_params.values().sum::<u32>(), 6 + 3 + 2);
        assert_eq!((c.idl_signatures, c.idl_from_idl), (1, 0));
    }

    #[test]
    fn idl_role_prose_blocks_are_sources_with_signatures() {
        use crate::state::ir::SourceContext;
        use crate::state::testing::{signature, ty, EVENT_DOM};
        let state = extract(EVENT_DOM, "DOM");
        let getter = state
            .sources
            .iter()
            .find(|s| s.subject.anchor == "dom-event-type")
            .expect("getter block is a source");
        assert!(matches!(getter.context, SourceContext::Prose { .. }));
        assert_eq!(
            ty(&signature(&state, "dom-event-type")
                .returns
                .as_ref()
                .unwrap()
                .ty),
            "idl-type:DOMString"
        );
        // Coverage counts algorithm signatures only: `initEvent`, not the getter.
        assert_eq!(state.coverage.idl_signatures, 1);
    }

    #[test]
    fn full_ir_is_extracted_for_the_event_fixture() {
        use crate::state::ir::{BlockRole, CallForm, Origin, StatementKind};
        use crate::state::testing::{calls_at, signature, statements_at, ty, EVENT_DOM};
        let state = extract(EVENT_DOM, "DOM");
        let step1 = statements_at(&state, "dom-event-initevent", "1");
        assert!(matches!(step1[0].kind, StatementKind::If { .. }));
        assert_eq!(step1[1].parent.as_ref().unwrap().role, BlockRole::Then);
        let call = calls_at(&state, "dom-event-initevent", "2")[0];
        assert_eq!(
            (
                call.form,
                call.callee.target.as_ref().unwrap().anchor.as_str()
            ),
            (CallForm::Imperative, "concept-event-initialize")
        );
        let init = signature(&state, "dom-event-initevent");
        assert_eq!(ty(init.this.as_ref().unwrap()), "idl:Event");
        let origins = state
            .var_origins
            .iter()
            .find(|v| v.subject.anchor == "dom-event-initevent")
            .unwrap();
        assert!(origins
            .vars
            .iter()
            .all(|v| matches!(v.origin, Origin::Param { .. })));
        let roles = state
            .link_roles
            .iter()
            .find(|r| r.source_id == call.source_id)
            .unwrap();
        assert_eq!(
            roles.roles.len(),
            crate::state::testing::source_at(&state, "dom-event-initevent", "2")
                .links
                .len()
        );
        let c = &state.coverage;
        assert_eq!((c.steps, c.steps_recognized), (3, 3));
        assert_eq!(c.step_heads.get("if"), Some(&1));
        assert_eq!(c.step_heads.get("call"), Some(&1));
        assert_eq!((c.if_total, c.if_parsed), (1, 1));
        assert_eq!(c.calls_by_form.get("imperative"), Some(&1));
        assert_eq!(c.expr_forms.get("return:none"), Some(&1));
        assert_eq!(c.expr_forms.get("return:path"), Some(&1));
        let summary = state
            .summaries
            .iter()
            .find(|s| s.anchor == "dom-event-initevent")
            .unwrap();
        assert_eq!((summary.statements.get("if"), summary.calls), (Some(&1), 1));
    }

    #[test]
    fn predicate_signatures_get_boolean_from_return_statements() {
        use crate::state::model::ReturnBasis;
        use crate::state::testing::{signature, ty, FALLBACK_HTML};
        let state = extract(FALLBACK_HTML, "HTML");
        let r = signature(&state, "same-origin").returns.clone().unwrap();
        assert_eq!(
            (ty(&r.ty), r.basis),
            ("boolean".to_string(), ReturnBasis::ReturnStatements)
        );
        assert_eq!(state.coverage.assert_total, 3);
        assert_eq!(state.coverage.assert_parsed, 2);
        assert_eq!(state.coverage.assert_review.len(), 1);
    }

    #[test]
    fn a_for_each_head_without_a_loop_counts_as_unbound() {
        let html = r##"<div data-algorithm=""><p>To <dfn id="shift">shift</dfn> given a <var>parent</var> and a <var>nodes</var>:</p><ol><li><p>For each <var>node</var> of <var>nodes</var>, set <var>node</var>'s <a href="#i">index</a> to 0.</p></li><li><p>For each live range whose start node is <var>parent</var>, set its start offset to 0.</p></li></ol></div>"##;
        let state = extract(html, "HTML");
        assert_eq!(
            (state.coverage.foreach_bound, state.coverage.foreach_total),
            (1, 2)
        );
    }

    #[test]
    fn set_to_be_calls_keep_their_opaque_write_site() {
        use crate::state::ir::{CallForm, StatementKind};
        use crate::state::model::SiteClass;
        let html = r##"<div data-algorithm=""><p>To <dfn id="blocked">be blocked</dfn> given a <var>d</var>:</p><ol><li><p>Return true.</p></li></ol></div>
<div data-algorithm=""><p>To <dfn id="mark">mark</dfn> given a <var>subject</var>:</p><ol><li><p>Set <var>subject</var>'s <a href="#nd">node document</a> to be <a href="#blocked">blocked</a>.</p></li></ol></div>"##;
        let state = extract(html, "HTML");
        let call = state
            .calls
            .iter()
            .find(|c| c.form == CallForm::SetToBe)
            .expect("the Set … to be clause is a SetToBe call");
        assert!(state
            .statements
            .iter()
            .any(|s| s.id == call.statement_id && matches!(s.kind, StatementKind::Call { .. })));
        let source = state
            .sources
            .iter()
            .find(|s| s.id == call.source_id)
            .unwrap();
        let after_set = source.text.find("Set ").unwrap() + "Set ".len();
        let expected_target =
            &source.text[after_set..after_set + source.text[after_set..].find(" to ").unwrap()];
        let opaque: Vec<_> = crate::state::derive_sites(&state)
            .into_iter()
            .filter(|s| s.class == SiteClass::OpaqueWrite)
            .collect();
        assert_eq!(
            opaque.len(),
            1,
            "exactly the site SP1 emitted for the opaque set"
        );
        assert_eq!(
            (opaque[0].op.as_str(), opaque[0].target_text.as_str()),
            ("set", expected_target)
        );
    }

    #[test]
    fn ecmarkup_and_empty_documents_do_not_panic() {
        let empty = extract("", "HTML");
        assert!(empty.model.fields.is_empty() && empty.sources.is_empty());
        let ecma = r#"<emu-clause id="sec-x" type="abstract operation"><h1><span class="secnum">7.1</span> X ( <var>O</var> )</h1><emu-alg><ol><li>Set <var>O</var>.[[Prototype]] to <emu-val>null</emu-val>.</li></ol></emu-alg></emu-clause>"#;
        let state = extract(ecma, "ECMA-262");
        assert!(state.statements.iter().any(|s| matches!(&s.kind, crate::state::ir::StatementKind::Set { targets, .. } if matches!(targets[0].hops[0], crate::state::ir::Hop::Slot { .. }))));
    }
}
