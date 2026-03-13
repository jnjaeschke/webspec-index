//! Integration tests for the analyze module.
//!
//! Uses fixture files with pre-loaded spec data (no database or network needed).

use std::collections::HashMap;

use webspec_index::analyze::file::{analyze_file, FileAnalysis, SpecResolver};
use webspec_index::analyze::matcher::MatchResult;
use webspec_index::analyze::scanner::SpecUrl;

/// Resolver backed by a JSON map of "SPEC#anchor" -> algorithm content.
struct FixtureResolver {
    sections: HashMap<String, String>,
}

impl FixtureResolver {
    fn from_json(json: &str) -> Self {
        let sections: HashMap<String, String> = serde_json::from_str(json).unwrap();
        FixtureResolver { sections }
    }
}

impl SpecResolver for FixtureResolver {
    fn resolve(&self, spec: &str, anchor: &str) -> Option<String> {
        let key = format!("{spec}#{anchor}");
        self.sections.get(&key).cloned()
    }
}

fn spec_urls() -> Vec<SpecUrl> {
    vec![
        SpecUrl {
            spec: "HTML".into(),
            base_url: "https://html.spec.whatwg.org".into(),
        },
        SpecUrl {
            spec: "DOM".into(),
            base_url: "https://dom.spec.whatwg.org".into(),
        },
    ]
}

const SPEC_DATA: &str = include_str!("fixtures/analyze/scoping/spec_data.json");
const THRESHOLD: f64 = 0.85;

fn analyze(fixture: &str) -> FileAnalysis {
    let resolver = FixtureResolver::from_json(SPEC_DATA);
    analyze_file(fixture, &spec_urls(), &resolver, THRESHOLD)
}

// ── Scoping: class members ──

#[test]
fn class_members_scope_closes_at_brace() {
    let text = include_str!("fixtures/analyze/scoping/class_members.cpp");
    let result = analyze(text);

    // Only one scope (navigate), steps 1-3.
    assert_eq!(result.scopes.len(), 1, "expected exactly 1 scope");
    let scope = &result.scopes[0];
    assert_eq!(scope.url_match.anchor, "navigate");
    assert_eq!(scope.validations.len(), 3, "expected 3 validated steps");

    // Step 4 in the unrelated function must NOT appear.
    let step_numbers: Vec<&Vec<u32>> = scope.validations.iter().map(|v| &v.step.number).collect();
    assert!(
        !step_numbers.contains(&&vec![4u32]),
        "step 4 should not be in scope"
    );

    // Steps 1 and 2 should be fuzzy matches (comment abbreviates spec).
    assert!(
        matches!(
            scope.validations[0].result,
            MatchResult::Fuzzy | MatchResult::Exact
        ),
        "step 1: expected fuzzy or exact, got {:?}",
        scope.validations[0].result
    );
    assert!(
        matches!(
            scope.validations[1].result,
            MatchResult::Fuzzy | MatchResult::Exact
        ),
        "step 2: expected fuzzy or exact, got {:?}",
        scope.validations[1].result
    );

    // Coverage: 3 steps total in the fixture spec, 3 implemented.
    let cov = scope.coverage.as_ref().unwrap();
    assert_eq!(cov.total_steps, 3);
    assert_eq!(cov.implemented_count(), 3);
}

// ── Scoping: nested algorithms ──

#[test]
fn nested_scopes_stack_and_unwind() {
    let text = include_str!("fixtures/analyze/scoping/nested.cpp");
    let result = analyze(text);

    assert_eq!(result.scopes.len(), 2, "expected 2 scopes (outer + inner)");

    // Outer scope: navigate, should have steps 1, 2, 3.
    let navigate = result
        .scopes
        .iter()
        .find(|s| s.url_match.anchor == "navigate")
        .expect("missing navigate scope");
    let nav_steps: Vec<&Vec<u32>> = navigate
        .validations
        .iter()
        .map(|v| &v.step.number)
        .collect();
    assert_eq!(nav_steps.len(), 3, "navigate should have 3 steps");
    assert!(nav_steps.contains(&&vec![1u32]));
    assert!(nav_steps.contains(&&vec![2u32]));
    assert!(nav_steps.contains(&&vec![3u32]));

    // Inner scope: concept-tree, should have steps 1, 2.
    let tree = result
        .scopes
        .iter()
        .find(|s| s.url_match.anchor == "concept-tree")
        .expect("missing concept-tree scope");
    let tree_steps: Vec<&Vec<u32>> = tree.validations.iter().map(|v| &v.step.number).collect();
    assert_eq!(tree_steps.len(), 2, "concept-tree should have 2 steps");
    assert!(tree_steps.contains(&&vec![1u32]));
    assert!(tree_steps.contains(&&vec![2u32]));
}

// ── Scoping: same-indent replacement ──

#[test]
fn same_indent_urls_create_separate_scopes() {
    let text = include_str!("fixtures/analyze/scoping/same_indent_replace.cpp");
    let result = analyze(text);

    assert_eq!(result.scopes.len(), 2, "expected 2 scopes");

    let navigate = result
        .scopes
        .iter()
        .find(|s| s.url_match.anchor == "navigate")
        .expect("missing navigate scope");
    assert_eq!(navigate.validations.len(), 1);
    assert_eq!(navigate.validations[0].step.number, vec![1]);

    let tree = result
        .scopes
        .iter()
        .find(|s| s.url_match.anchor == "concept-tree")
        .expect("missing concept-tree scope");
    assert_eq!(tree.validations.len(), 1);
    assert_eq!(tree.validations[0].step.number, vec![1]);
}

// ── Validation correctness ──

#[test]
fn validation_results_are_correct() {
    let text = "\
// https://html.spec.whatwg.org/#navigate
void foo() {
    // Step 1. Let cspNavigationType be form-submission
    code();
    // Step 99. This step does not exist
    code();
}
";
    let result = analyze(text);
    assert_eq!(result.scopes.len(), 1);
    let scope = &result.scopes[0];
    assert_eq!(scope.validations.len(), 2);

    // Step 1 should be fuzzy (comment abbreviates spec).
    assert!(matches!(
        scope.validations[0].result,
        MatchResult::Fuzzy | MatchResult::Exact
    ));

    // Step 99 should be not_found.
    assert_eq!(scope.validations[1].result, MatchResult::NotFound);
}

// ── No spec URLs: empty result ──

#[test]
fn no_spec_urls_produces_empty_result() {
    let text = "void foo() { code(); }";
    let result = analyze(text);
    assert!(result.scopes.is_empty());
}

// ── Unknown section: no validations ──

#[test]
fn unknown_section_produces_no_validations() {
    let text = "\
// https://html.spec.whatwg.org/#nonexistent-section
void foo() {
    // Step 1. Something
    code();
}
";
    let result = analyze(text);
    assert_eq!(result.scopes.len(), 1);
    assert!(result.scopes[0].validations.is_empty());
    assert!(result.scopes[0].coverage.is_none());
}
