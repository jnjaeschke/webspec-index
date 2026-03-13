//! High-level file analysis: scan, scope, validate, and compute coverage.
//!
//! Returns rich domain types ([`FileAnalysis`], [`ScopeAnalysis`]) suitable for
//! both the LSP server and the CLI.  Serializable "view" types are provided for
//! JSON / searchfox output via [`FileAnalysisView`].

use serde::Serialize;

use super::coverage::{compute_coverage, CoverageResult, StepValidation};
use super::matcher::{classify_match, MatchResult};
use super::scanner::{
    build_scopes, build_spec_lookup, build_url_pattern, scan_document, scan_steps, SpecUrl,
    UrlMatch,
};
use super::steps::{find_step, parse_steps};

// ── Rich domain types (used by LSP + CLI) ────────────────────────────

/// Result of analyzing a single source file.
#[derive(Debug, Clone)]
pub struct FileAnalysis {
    /// All spec URL matches found in the file (for position lookups).
    pub url_matches: Vec<UrlMatch>,
    /// Per-scope analysis results.
    pub scopes: Vec<ScopeAnalysis>,
}

/// Analysis of a single spec scope within a file.
#[derive(Debug, Clone)]
pub struct ScopeAnalysis {
    pub url_match: UrlMatch,
    pub validations: Vec<StepValidation>,
    pub coverage: Option<CoverageResult>,
}

// ── Serializable view types (for JSON / searchfox output) ────────────

/// Serializable view of [`FileAnalysis`].
#[derive(Debug, Serialize)]
pub struct FileAnalysisView {
    pub scopes: Vec<ScopeAnalysisView>,
}

/// Serializable view of [`ScopeAnalysis`].
#[derive(Debug, Serialize)]
pub struct ScopeAnalysisView {
    pub spec: String,
    pub anchor: String,
    pub url: String,
    pub line: usize,
    pub col: usize,
    pub validations: Vec<StepAnalysisView>,
    pub coverage: Option<CoverageSummary>,
}

/// Serializable view of a single step validation.
#[derive(Debug, Serialize)]
pub struct StepAnalysisView {
    pub line: usize,
    pub col: usize,
    pub step: Vec<u32>,
    pub comment_text: String,
    pub result: String,
    pub spec_text: String,
}

/// Coverage summary for a scope.
#[derive(Debug, Serialize)]
pub struct CoverageSummary {
    pub total: usize,
    pub implemented: usize,
    pub missing: Vec<Vec<u32>>,
    pub warnings: usize,
    pub reordered: usize,
}

impl From<&CoverageResult> for CoverageSummary {
    fn from(cr: &CoverageResult) -> Self {
        CoverageSummary {
            total: cr.total_steps,
            implemented: cr.implemented_count(),
            missing: cr.missing.clone(),
            warnings: cr.warnings,
            reordered: cr.reordered,
        }
    }
}

impl From<&FileAnalysis> for FileAnalysisView {
    fn from(fa: &FileAnalysis) -> Self {
        FileAnalysisView {
            scopes: fa.scopes.iter().map(ScopeAnalysisView::from).collect(),
        }
    }
}

impl From<&ScopeAnalysis> for ScopeAnalysisView {
    fn from(sa: &ScopeAnalysis) -> Self {
        ScopeAnalysisView {
            spec: sa.url_match.spec.clone(),
            anchor: sa.url_match.anchor.clone(),
            url: sa.url_match.url.clone(),
            line: sa.url_match.line,
            col: sa.url_match.indent,
            validations: sa.validations.iter().map(StepAnalysisView::from).collect(),
            coverage: sa.coverage.as_ref().map(CoverageSummary::from),
        }
    }
}

impl From<&StepValidation> for StepAnalysisView {
    fn from(sv: &StepValidation) -> Self {
        StepAnalysisView {
            line: sv.step.line,
            col: sv.step.indent,
            step: sv.step.number.clone(),
            comment_text: sv.step.text.clone(),
            result: sv.result.as_str().to_string(),
            spec_text: sv.spec_text.clone(),
        }
    }
}

// ── Spec resolution trait ────────────────────────────────────────────

/// Resolve a spec section's algorithm content.
///
/// Implementors provide access to spec data — either from a database or from
/// pre-loaded JSON fixtures.
pub trait SpecResolver {
    /// Return the algorithm markdown content for a spec section, if available.
    fn resolve(&self, spec: &str, anchor: &str) -> Option<String>;
}

// ── Core analysis ────────────────────────────────────────────────────

/// Analyze a source file against spec data.
///
/// Scans `text` for spec URLs and step comments, builds indentation-based
/// scopes, validates each step against the spec algorithm, and computes
/// coverage metrics.
pub fn analyze_file(
    text: &str,
    spec_urls: &[SpecUrl],
    resolver: &dyn SpecResolver,
    threshold: f64,
) -> FileAnalysis {
    let pattern = build_url_pattern(spec_urls);
    let spec_lookup = build_spec_lookup(spec_urls);
    let url_matches = scan_document(text, &pattern, &spec_lookup);
    let step_comments = scan_steps(text);
    let scopes = build_scopes(text, &url_matches, &step_comments);

    let mut scope_results = Vec::new();

    for (url_match, steps_in_scope) in &scopes {
        let content = resolver.resolve(&url_match.spec, &url_match.anchor);

        let algo_steps = content
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(parse_steps);

        let mut validations = Vec::new();

        for sc in steps_in_scope {
            let (match_result, spec_text) = if let Some(ref steps) = algo_steps {
                if let Some(ss) = find_step(steps, &sc.number) {
                    (
                        classify_match(&sc.text, &ss.text, threshold),
                        ss.text.clone(),
                    )
                } else {
                    (MatchResult::NotFound, String::new())
                }
            } else {
                // No algorithm content available — can't validate.
                continue;
            };

            validations.push(StepValidation {
                step: sc.clone(),
                result: match_result,
                spec_text,
                algo_anchor: url_match.anchor.clone(),
            });
        }

        let coverage = algo_steps
            .as_deref()
            .map(|steps| compute_coverage(&validations, steps, &url_match.anchor));

        scope_results.push(ScopeAnalysis {
            url_match: url_match.clone(),
            validations,
            coverage,
        });
    }

    FileAnalysis {
        url_matches,
        scopes: scope_results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeResolver {
        sections: std::collections::HashMap<(String, String), String>,
    }

    impl FakeResolver {
        fn new() -> Self {
            let mut sections = std::collections::HashMap::new();
            sections.insert(
                ("HTML".to_string(), "navigate".to_string()),
                "1. Let *cspNavigationType* be \"`form-submission`\" if *formDataEntryList* is non-null; otherwise \"`other`\".\n\
                 2. Let *sourceSnapshotParams* be the result of snapshotting source snapshot params given *sourceDocument*.\n\
                 3. If *url* is `about:blank`, then return.".to_string(),
            );
            FakeResolver { sections }
        }
    }

    impl SpecResolver for FakeResolver {
        fn resolve(&self, spec: &str, anchor: &str) -> Option<String> {
            self.sections
                .get(&(spec.to_string(), anchor.to_string()))
                .cloned()
        }
    }

    fn spec_urls() -> Vec<SpecUrl> {
        vec![SpecUrl {
            spec: "HTML".into(),
            base_url: "https://html.spec.whatwg.org".into(),
        }]
    }

    #[test]
    fn analyze_simple_file() {
        let text = "\
// https://html.spec.whatwg.org/#navigate
void DoNavigate() {
  // Step 1. Let cspNavigationType be form-submission
  auto csp = GetCSPNavType();

  // Step 2. Let sourceSnapshotParams be the result of snapshotting
  auto params = Snapshot();

  // Step 3. If url is about:blank, then return
  if (IsAboutBlank(url)) {
    return;
  }
}
";
        let result = analyze_file(text, &spec_urls(), &FakeResolver::new(), 0.85);
        assert_eq!(result.scopes.len(), 1);

        let scope = &result.scopes[0];
        assert_eq!(scope.url_match.anchor, "navigate");
        assert_eq!(scope.validations.len(), 3);

        assert!(matches!(
            scope.validations[0].result,
            MatchResult::Fuzzy | MatchResult::Exact
        ));
        assert_ne!(scope.validations[2].result, MatchResult::NotFound);

        let cov = scope.coverage.as_ref().unwrap();
        assert_eq!(cov.total_steps, 3);
        assert_eq!(cov.implemented_count(), 3);
        assert!(cov.missing.is_empty());
    }

    #[test]
    fn analyze_with_not_found_step() {
        let text = "\
// https://html.spec.whatwg.org/#navigate
void DoNavigate() {
  // Step 99. Nonexistent step
  DoSomething();
}
";
        let result = analyze_file(text, &spec_urls(), &FakeResolver::new(), 0.85);
        assert_eq!(result.scopes[0].validations.len(), 1);
        assert_eq!(
            result.scopes[0].validations[0].result,
            MatchResult::NotFound
        );
        assert_eq!(result.scopes[0].coverage.as_ref().unwrap().warnings, 1);
    }

    #[test]
    fn analyze_no_spec_urls() {
        let text = "void foo() { code(); }";
        let result = analyze_file(text, &spec_urls(), &FakeResolver::new(), 0.85);
        assert!(result.scopes.is_empty());
    }

    #[test]
    fn analyze_unknown_section() {
        let text = "\
// https://html.spec.whatwg.org/#nonexistent-section
void foo() {
  // Step 1. Something
  code();
}
";
        let result = analyze_file(text, &spec_urls(), &FakeResolver::new(), 0.85);
        assert_eq!(result.scopes.len(), 1);
        assert!(result.scopes[0].validations.is_empty());
        assert!(result.scopes[0].coverage.is_none());
    }

    #[test]
    fn analyze_scoping_isolates_functions() {
        let text = "\
class Foo {
  // https://html.spec.whatwg.org/#navigate
  void navigate() {
    // Step 1. Let cspNavigationType be form-submission
    code();
  }

  void other() {
    // Step 2. This should NOT be in navigate scope
    other_code();
  }
}
";
        let result = analyze_file(text, &spec_urls(), &FakeResolver::new(), 0.85);
        assert_eq!(result.scopes.len(), 1);
        assert_eq!(result.scopes[0].validations.len(), 1);
        assert_eq!(result.scopes[0].validations[0].step.number, vec![1]);
    }

    #[test]
    fn view_roundtrip() {
        let text = "\
// https://html.spec.whatwg.org/#navigate
void foo() {
  // Step 1. Let cspNavigationType be form-submission
  code();
}
";
        let result = analyze_file(text, &spec_urls(), &FakeResolver::new(), 0.85);
        let view = FileAnalysisView::from(&result);
        assert_eq!(view.scopes.len(), 1);
        assert_eq!(view.scopes[0].anchor, "navigate");
        assert_eq!(view.scopes[0].validations.len(), 1);
        assert!(
            view.scopes[0].validations[0].result == "fuzzy"
                || view.scopes[0].validations[0].result == "exact"
        );
    }
}
