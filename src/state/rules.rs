//! Reviewed verb rules and declared writes (§8.3), applied to each statement
//! source after the grammar. A rule never deletes grammar output: it positions
//! links the grammar left without a role, and attaches its id to the writes
//! and inits the grammar already found.
use crate::parse::steps::{AnchorTarget, TextSpan};
use crate::semantics::{scoped_text_matches, Anchor};
use crate::state::catalog::{StateEmit, StateRule};
use crate::state::classify::fallback_class;
use crate::state::extract::{site, site_id, SiteParts};
use crate::state::ir::{
    self, MutationOp, OpBasis, ParsedSource, SourceContext, Statement, StatementKind,
    StatementSource,
};
use crate::state::model::{Occurrence, OccurrenceClass, Site, SiteClass, StateCatalog};

/// One match of a rule in a source: the matched span, and the captures of the
/// rule's `text` (none for an anchor rule without `text`).
struct RuleMatch<'r, 't> {
    rule: &'r StateRule,
    span: TextSpan,
    captures: Option<regex::Captures<'t>>,
}

/// Apply the catalog's rules to one algorithm or prose source. `occurrences`
/// are the grammar's occurrences of `source`; rule statements go into
/// `parsed`, `Declared` sites of write/init rules that matched no link into
/// `declared`.
pub(crate) fn apply_rules(
    catalog: &StateCatalog,
    source: &StatementSource,
    parsed: &mut ParsedSource,
    occurrences: &mut [Occurrence],
    declared: &mut Vec<Site>,
) {
    if catalog.rules.is_empty() || matches!(source.context, SourceContext::BranchLabel { .. }) {
        return;
    }
    for found in rule_matches(&catalog.rules, source) {
        match &found.rule.emit {
            StateEmit::Mutate {
                op,
                target,
                operand,
            } => apply_mutate(
                source,
                parsed,
                occurrences,
                &found,
                op,
                target,
                operand.as_deref(),
            ),
            StateEmit::Write { field, op } => apply_declared(
                source,
                occurrences,
                &found,
                field,
                op,
                OccurrenceClass::Write,
                declared,
            ),
            StateEmit::Init { field } => apply_declared(
                source,
                occurrences,
                &found,
                field,
                "init",
                OccurrenceClass::Init,
                declared,
            ),
        }
    }
}

/// Anchorless rules through `scoped_text_matches`; an anchor rule matches at
/// each link targeting its anchor, where its `text` (if any) covers the link.
fn rule_matches<'r, 't>(
    rules: &'r [StateRule],
    source: &'t StatementSource,
) -> Vec<RuleMatch<'r, 't>> {
    let subject = Anchor {
        spec: source.subject.spec.clone(),
        anchor: source.subject.anchor.clone(),
    };
    let text = source.text.as_str();
    let mut matches: Vec<RuleMatch> =
        scoped_text_matches(rules.iter().map(|r| (r, &r.match_spec)), &subject, text)
            .into_iter()
            .flat_map(|((start, end), hits)| {
                hits.into_iter().map(move |(rule, captures)| RuleMatch {
                    rule,
                    span: TextSpan { start, end },
                    captures: Some(captures),
                })
            })
            .collect();
    for rule in rules {
        let spec = &rule.match_spec;
        let Some(anchor) = &spec.anchor else {
            continue;
        };
        if spec.subject.as_ref().is_some_and(|s| *s != subject)
            || spec.exclude_text.iter().any(|p| p.regex().is_match(text))
        {
            continue;
        }
        let targets_anchor = |target: &AnchorTarget| {
            target.spec.eq_ignore_ascii_case(&anchor.spec) && target.anchor == anchor.anchor
        };
        for link in &source.links {
            if !link.target.as_ref().is_some_and(targets_anchor) {
                continue;
            }
            let Some(pattern) = &spec.text else {
                matches.push(RuleMatch {
                    rule,
                    span: link.span,
                    captures: None,
                });
                continue;
            };
            let covering = pattern.regex().captures_iter(text).find(|captures| {
                captures.get(0).is_some_and(|whole| {
                    whole.start() <= link.span.start && link.span.end <= whole.end()
                })
            });
            if let Some(captures) = covering {
                let whole = captures.get(0).expect("group 0 always participates");
                matches.push(RuleMatch {
                    rule,
                    span: TextSpan {
                        start: whole.start(),
                        end: whole.end(),
                    },
                    captures: Some(captures),
                });
            }
        }
    }
    matches
}

fn mutation_op(op: &str) -> Option<MutationOp> {
    serde_json::from_value(serde_json::Value::String(op.to_owned())).ok()
}

fn capture_span(captures: &regex::Captures, name: &str) -> Option<TextSpan> {
    captures.name(name).map(|m| TextSpan {
        start: m.start(),
        end: m.end(),
    })
}

/// `state.mutate`: the target capture parsed as a `PATH` gives a `Mutate`
/// statement over the covering clause, which the rule consumes.
fn apply_mutate(
    source: &StatementSource,
    parsed: &mut ParsedSource,
    occurrences: &mut [Occurrence],
    found: &RuleMatch,
    op: &str,
    target: &str,
    operand: Option<&str>,
) {
    let Some(captures) = &found.captures else {
        return;
    };
    let (Some(target_span), Some(op)) = (capture_span(captures, target), mutation_op(op)) else {
        return;
    };
    let Some((path, roles)) = ir::parse_path_span(source, target_span) else {
        return;
    };
    let operand_span = operand.and_then(|name| capture_span(captures, name));
    let first = operand_span.map_or(target_span.start, |o| o.start.min(target_span.start));
    let clause = parsed.clauses.iter().rposition(|c| c.start <= first);
    let span = TextSpan {
        start: clause.map_or(found.span.start, |i| parsed.clauses[i].start),
        end: target_span.end,
    };
    let rule_id = &found.rule.public_id;
    let id = ir::statement_id(&source.id, "mutate", span);
    if !parsed.statements.iter().any(|s| s.id == id) {
        parsed.statements.push(Statement {
            id: id.clone(),
            source_id: source.id.clone(),
            span,
            kind: StatementKind::Mutate {
                op,
                target: path,
                operand: operand_span.map(|s| ir::parse_expr_span(source, s)),
                basis: OpBasis::Rule {
                    rule_id: rule_id.clone(),
                },
            },
            parent: None,
        });
    }
    let operand_links = source
        .links
        .iter()
        .enumerate()
        .filter(|(_, link)| {
            operand_span.is_some_and(|s| s.start <= link.span.start && link.span.end <= s.end)
        })
        .map(|(index, _)| (index, OccurrenceClass::Read));
    let positioned: Vec<(usize, OccurrenceClass)> = roles
        .write
        .map(|link| (link, OccurrenceClass::Write))
        .into_iter()
        .chain(
            roles
                .read_path
                .iter()
                .map(|&link| (link, OccurrenceClass::ReadPath)),
        )
        .chain(roles.read.iter().map(|&link| (link, OccurrenceClass::Read)))
        .chain(operand_links)
        .collect();
    let basis = format!("rule:{rule_id}");
    for (link, class) in positioned {
        let Some(occurrence) = occurrence_of(source, occurrences, link) else {
            continue;
        };
        match parsed.link_roles.get(&link) {
            Some((OccurrenceClass::Write | OccurrenceClass::Init, _))
                if class == OccurrenceClass::Write =>
            {
                attach(occurrence, rule_id);
            }
            Some(_) => {}
            None => {
                occurrence.class = class;
                occurrence.statement_id = Some(id.clone());
                occurrence.basis = basis.clone();
                parsed.link_roles.insert(link, (class, id.clone()));
            }
        }
    }
    if let Some(index) = clause {
        parsed.clauses[index].consumed = true;
    }
    for (index, link) in source.links.iter().enumerate() {
        if parsed.link_roles.contains_key(&index) {
            continue;
        }
        if let Some(occurrence) = occurrence_of(source, occurrences, index)
            .filter(|o| o.class == OccurrenceClass::Unclassified)
        {
            occurrence.class = fallback_class(source, parsed, link.span.start);
        }
    }
}

fn occurrence_of<'o>(
    source: &StatementSource,
    occurrences: &'o mut [Occurrence],
    link: usize,
) -> Option<&'o mut Occurrence> {
    let id = &source.links.get(link)?.id;
    occurrences.iter_mut().find(|o| o.link_id == *id)
}

fn attach(occurrence: &mut Occurrence, rule_id: &str) {
    if !occurrence.rule_ids.iter().any(|id| id == rule_id) {
        occurrence.rule_ids.push(rule_id.to_owned());
    }
}

/// `state.write` / `state.init`: reclassify the unclassified occurrences of
/// `field` in the matched span, attach the rule to its grammar writes and
/// inits there, or else record a `Declared` site.
fn apply_declared(
    source: &StatementSource,
    occurrences: &mut [Occurrence],
    found: &RuleMatch,
    field: &AnchorTarget,
    op: &str,
    class: OccurrenceClass,
    declared: &mut Vec<Site>,
) {
    let rule_id = &found.rule.public_id;
    let basis = format!("rule:{rule_id}");
    let mut matched = false;
    for occurrence in occurrences.iter_mut() {
        if occurrence.target.as_ref() != Some(field) {
            continue;
        }
        let inside = source
            .links
            .iter()
            .find(|link| link.id == occurrence.link_id)
            .is_some_and(|link| {
                found.span.start <= link.span.start && link.span.end <= found.span.end
            });
        if !inside {
            continue;
        }
        match occurrence.class {
            OccurrenceClass::Unclassified => {
                occurrence.class = class;
                occurrence.basis = basis.clone();
                matched = true;
            }
            OccurrenceClass::Write | OccurrenceClass::Init => {
                attach(occurrence, rule_id);
                matched = true;
            }
            OccurrenceClass::Read | OccurrenceClass::ReadPath => {}
        }
    }
    if matched {
        return;
    }
    let text = source.text[found.span.start..found.span.end]
        .trim()
        .to_string();
    let local_id = format!("{rule_id}\0{}", found.span.start);
    let mut row = site(
        source,
        text,
        site_id(&source.id, &local_id, SiteClass::Declared),
        SiteClass::Declared,
        Some(field.clone()),
        SiteParts {
            op: op.to_owned(),
            receiver: "opaque",
            constructed: None,
            target_text: format!("{}#{}", field.spec, field.anchor),
            value_text: None,
        },
        &basis,
        Some(found.span),
    );
    row.segment_id = None;
    row.body_id = None;
    declared.push(row);
}

#[cfg(test)]
mod tests {
    use crate::state::catalog::load_state_files;
    use crate::state::extract::derive_sites;
    use crate::state::ir::{OpBasis, StatementKind};
    use crate::state::model::{OccurrenceClass, SiteClass};
    use crate::state::testing::{extract_with_catalog, ADD_RULE_YAML};

    const DIALOG: &str = r##"<div class="algorithm"><p>To <dfn id="dialog-setup-steps">set up</dfn>:</p><ol><li><p>Add <var>subject</var> to <var>subject</var>'s <a href="https://dom.spec.whatwg.org/#concept-node-document">node document</a>'s <a href="#open-dialogs-list">open dialogs list</a>.</p></li>
      <li><p>Add <var>x</var> to <var>list</var>.</p></li></ol></div>"##;

    #[test]
    fn add_rule_turns_unclassified_into_a_write_on_the_last_hop() {
        let catalog = load_state_files(&[("state/r.yaml", ADD_RULE_YAML)]).unwrap();
        let state = extract_with_catalog(DIALOG, "HTML", &catalog);
        let class = |anchor: &str| {
            state
                .occurrences
                .iter()
                .find(|o| o.target.as_ref().is_some_and(|t| t.anchor == anchor))
                .map(|o| (o.class, o.basis.clone()))
        };
        assert_eq!(
            class("open-dialogs-list"),
            Some((
                OccurrenceClass::Write,
                "rule:webspec-index/add-to-field-collection".into()
            ))
        );
        assert_eq!(
            class("concept-node-document").unwrap().0,
            OccurrenceClass::ReadPath
        );
        let sites = derive_sites(&state);
        assert!(!sites
            .iter()
            .any(|s| s.class == SiteClass::OpaqueWrite && s.step_path.as_deref() == Some("1")));
        let write = sites
            .iter()
            .find(|s| s.class == SiteClass::Write)
            .expect("rule write site");
        assert_eq!(
            (
                write.op.as_str(),
                write.target_text.as_str(),
                write.value_text.as_deref()
            ),
            (
                "append",
                "*subject*'s node document's open dialogs list",
                Some("*subject*")
            )
        );
        assert!(state.statements.iter().any(|s| matches!(
            &s.kind,
            StatementKind::Mutate { basis: OpBasis::Rule { rule_id }, .. }
                if rule_id == "webspec-index/add-to-field-collection"
        )));
    }

    #[test]
    fn local_accumulator_is_excluded_and_keeps_its_opaque_write() {
        let catalog = load_state_files(&[("state/r.yaml", ADD_RULE_YAML)]).unwrap();
        let sites = derive_sites(&extract_with_catalog(DIALOG, "HTML", &catalog));
        assert!(sites
            .iter()
            .any(|s| s.class == SiteClass::OpaqueWrite && s.step_path.as_deref() == Some("2")));
    }

    #[test]
    fn grammar_write_keeps_its_class_and_gains_the_rule_id() {
        let yaml = "schema: 1\npackage: p\nrules:\n  - id: w\n    match: {text: 'Set (?P<t>.+?) to '}\n    emit: {kind: state.mutate, params: {op: append, target: {capture: t, from: path}}}\n";
        let catalog = load_state_files(&[("state/w.yaml", yaml)]).unwrap();
        let html = r##"<div data-algorithm=""><p>To <dfn id="run">run</dfn> given <var>d</var>:</p><ol>
<li><p>Set <var>d</var>'s <a href="#p">p</a> to 1.</p></li></ol></div>"##;
        let state = extract_with_catalog(html, "HTML", &catalog);
        let occurrence = state
            .occurrences
            .iter()
            .find(|o| o.target.as_ref().is_some_and(|t| t.anchor == "p"))
            .unwrap();
        assert_eq!(
            (occurrence.class, occurrence.basis.as_str()),
            (OccurrenceClass::Write, "ir")
        );
        assert_eq!(occurrence.rule_ids, ["p/w"]);
    }

    #[test]
    fn write_rules_reclassify_in_span_or_declare_a_site() {
        let yaml = "schema: 1\npackage: p\nrules:\n  - id: reset\n    match: {text: 'Reset \\S+ to '}\n    emit: {kind: state.write, params: {field: HTML#m, op: set}}\n  - id: other\n    match: {text: 'Reset '}\n    emit: {kind: state.write, params: {field: HTML#unlinked, op: set}}\n";
        let catalog = load_state_files(&[("state/w.yaml", yaml)]).unwrap();
        let html = r##"<div data-algorithm=""><p>To <dfn id="run">run</dfn>:</p><ol>
<li><p>Reset <a href="#m">m</a> to the identity.</p></li></ol></div>"##;
        let state = extract_with_catalog(html, "HTML", &catalog);
        let occurrence = state
            .occurrences
            .iter()
            .find(|o| o.target.as_ref().is_some_and(|t| t.anchor == "m"))
            .unwrap();
        assert_eq!(
            (occurrence.class, occurrence.basis.as_str()),
            (OccurrenceClass::Write, "rule:p/reset")
        );
        let sites = derive_sites(&state);
        let write = sites.iter().find(|s| s.class == SiteClass::Write).unwrap();
        assert_eq!(write.op, "reset");
        let declared = sites
            .iter()
            .find(|s| s.class == SiteClass::Declared)
            .expect("declared site");
        assert_eq!(
            (
                declared.target.as_ref().map(|t| t.anchor.as_str()),
                declared.op.as_str(),
                declared.basis.as_str(),
                declared.receiver.as_str(),
                declared.text.as_str(),
                declared.step_path.as_deref(),
                declared.segment_id.as_deref()
            ),
            (
                Some("unlinked"),
                "set",
                "rule:p/other",
                "opaque",
                "Reset",
                Some("1"),
                None
            )
        );
    }
}
