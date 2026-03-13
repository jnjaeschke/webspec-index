//! Searchfox analysis JSON output format.
//!
//! Converts [`FileAnalysisView`] results into searchfox-compatible analysis
//! records (line-delimited JSON). Each record is either a "source" record
//! (controls display and context menus) or a "target" record (drives
//! cross-referencing).
//!
//! See `mozsearch/docs/analysis.md` for the full format spec.

use serde::Serialize;

use super::file::{FileAnalysisView, ScopeAnalysisView, StepAnalysisView};

/// A searchfox source record.
#[derive(Serialize)]
struct SourceRecord<'a> {
    /// Location: "line:start_col-end_col" (1-based lines, 0-based cols).
    loc: String,
    source: u8,
    syntax: &'a str,
    pretty: String,
    sym: String,
}

/// A searchfox target record.
#[derive(Serialize)]
struct TargetRecord<'a> {
    /// Location: "line:col" (1-based lines, 0-based cols).
    loc: String,
    target: u8,
    kind: &'a str,
    pretty: String,
    sym: String,
}

/// Format a spec symbol identifier.
///
/// Convention: `SPEC_<SPEC>_<anchor>` — uses underscores since searchfox
/// symbol names use `#` for JS property notation.
fn spec_symbol(spec: &str, anchor: &str) -> String {
    format!("SPEC_{spec}_{anchor}")
}

/// Convert a `FileAnalysisView` into searchfox analysis JSON lines.
///
/// Returns a string with one JSON record per line (no trailing newline on
/// the last record, matching searchfox convention).
pub fn to_searchfox_records(analysis: &FileAnalysisView) -> String {
    let mut lines: Vec<String> = Vec::new();

    for scope in &analysis.scopes {
        emit_scope_records(scope, &mut lines);
    }

    lines.join("\n")
}

fn emit_scope_records(scope: &ScopeAnalysisView, lines: &mut Vec<String>) {
    let sym = spec_symbol(&scope.spec, &scope.anchor);
    // Searchfox uses 1-based line numbers, 0-based columns.
    let line1 = scope.line + 1;
    let col = scope.col;

    // Source record for the spec URL comment.
    // Column must match the comment token's start position (its indent).
    let source = SourceRecord {
        loc: format!("{line1}:{col}"),
        source: 1,
        syntax: "use",
        pretty: format!("spec {}#{}", scope.spec, scope.anchor),
        sym: sym.clone(),
    };
    lines.push(serde_json::to_string(&source).unwrap());

    // Target record: marks this location as a "use" of the spec symbol.
    // This feeds into crossref so all files referencing the same spec
    // algorithm can be found.
    let target = TargetRecord {
        loc: format!("{line1}:{col}"),
        target: 1,
        kind: "use",
        pretty: format!("{}#{}", scope.spec, scope.anchor),
        sym: sym.clone(),
    };
    lines.push(serde_json::to_string(&target).unwrap());

    // Source records for each step comment.
    for step in &scope.validations {
        emit_step_records(step, &sym, lines);
    }
}

fn emit_step_records(step: &StepAnalysisView, scope_sym: &str, lines: &mut Vec<String>) {
    let line1 = step.line + 1;
    let col = step.col;
    let step_num = step
        .step
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(".");

    // Choose syntax class based on validation result.
    // "def" makes it bold (good for matches), "use" for normal.
    // We also embed the result in `pretty` since searchfox doesn't have
    // native support for spec validation styling yet.
    let (syntax, indicator) = match step.result.as_str() {
        "exact" => ("def", "\u{2713}"),
        "fuzzy" => ("def", "~"),
        "mismatch" => ("type", "\u{2717}"),
        "not_found" => ("type", "?"),
        _ => ("use", ""),
    };

    let pretty = if step.spec_text.is_empty() {
        format!("Step {step_num} [{indicator} {}]", step.result)
    } else {
        format!(
            "Step {step_num} [{indicator} {}] spec: {}",
            step.result, step.spec_text
        )
    };

    let source = SourceRecord {
        loc: format!("{line1}:{col}"),
        source: 1,
        syntax,
        pretty,
        sym: scope_sym.to_string(),
    };
    lines.push(serde_json::to_string(&source).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::file::{
        CoverageSummary, FileAnalysisView, ScopeAnalysisView, StepAnalysisView,
    };

    fn make_analysis() -> FileAnalysisView {
        FileAnalysisView {
            scopes: vec![ScopeAnalysisView {
                spec: "HTML".to_string(),
                anchor: "navigate".to_string(),
                url: "https://html.spec.whatwg.org/#navigate".to_string(),
                line: 0,
                col: 0,
                validations: vec![
                    StepAnalysisView {
                        line: 2,
                        col: 4,
                        step: vec![1],
                        comment_text: "Let csp be form-submission".to_string(),
                        result: "fuzzy".to_string(),
                        spec_text: "Let cspNavigationType be form-submission".to_string(),
                    },
                    StepAnalysisView {
                        line: 5,
                        col: 4,
                        step: vec![99],
                        comment_text: "Nonexistent".to_string(),
                        result: "not_found".to_string(),
                        spec_text: String::new(),
                    },
                ],
                coverage: Some(CoverageSummary {
                    total: 3,
                    implemented: 1,
                    missing: vec![vec![2], vec![3]],
                    warnings: 1,
                    reordered: 0,
                }),
            }],
        }
    }

    #[test]
    fn generates_source_and_target_for_url() {
        let output = to_searchfox_records(&make_analysis());
        let records: Vec<serde_json::Value> = output
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        // First record: source for the URL.
        assert_eq!(records[0]["source"], 1);
        assert_eq!(records[0]["sym"], "SPEC_HTML_navigate");
        assert!(records[0]["pretty"]
            .as_str()
            .unwrap()
            .contains("spec HTML#navigate"));

        // Second record: target for the URL.
        assert_eq!(records[1]["target"], 1);
        assert_eq!(records[1]["kind"], "use");
        assert_eq!(records[1]["sym"], "SPEC_HTML_navigate");
    }

    #[test]
    fn generates_step_records() {
        let output = to_searchfox_records(&make_analysis());
        let records: Vec<serde_json::Value> = output
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        // 2 records for URL (source + target) + 2 step source records = 4
        assert_eq!(records.len(), 4);

        // Fuzzy match step: bold ("def"), includes spec text.
        let step1 = &records[2];
        assert_eq!(step1["syntax"], "def");
        assert!(step1["pretty"].as_str().unwrap().contains("Step 1"));
        assert!(step1["pretty"].as_str().unwrap().contains("fuzzy"));

        // Not-found step: "type" syntax (different color), no spec text.
        let step2 = &records[3];
        assert_eq!(step2["syntax"], "type");
        assert!(step2["pretty"].as_str().unwrap().contains("Step 99"));
        assert!(step2["pretty"].as_str().unwrap().contains("not_found"));
    }

    #[test]
    fn line_numbers_are_1_based() {
        let output = to_searchfox_records(&make_analysis());
        let records: Vec<serde_json::Value> = output
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        // URL is at line 0, col 0 → "1:0" in output.
        assert_eq!(records[0]["loc"], "1:0");
        // Step at line 2, col 4 → "3:4".
        assert_eq!(records[2]["loc"], "3:4");
        // Step at line 5, col 4 → "6:4".
        assert_eq!(records[3]["loc"], "6:4");
    }

    #[test]
    fn spec_symbol_format() {
        assert_eq!(spec_symbol("HTML", "navigate"), "SPEC_HTML_navigate");
        assert_eq!(spec_symbol("DOM", "concept-tree"), "SPEC_DOM_concept-tree");
    }

    #[test]
    fn empty_analysis_produces_no_output() {
        let analysis = FileAnalysisView { scopes: vec![] };
        assert!(to_searchfox_records(&analysis).is_empty());
    }
}
