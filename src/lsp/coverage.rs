//! Coverage computation for spec algorithm step tracking.

use super::matcher::MatchResult;
use super::scanner::StepComment;
use super::steps::{flatten_steps, AlgorithmStep};

/// Coverage of a spec algorithm in source code.
#[derive(Debug, Clone)]
pub struct CoverageResult {
    pub anchor: String,
    pub total_steps: usize,
    pub implemented: Vec<Vec<u32>>,
    pub missing: Vec<Vec<u32>>,
    pub warnings: usize,
    pub reordered: usize,
}

impl CoverageResult {
    pub fn implemented_count(&self) -> usize {
        self.implemented.len()
    }

    /// One-line summary for code lens display.
    pub fn summary(&self) -> String {
        let mut parts = vec![format!(
            "{}: {}/{} steps",
            self.anchor,
            self.implemented_count(),
            self.total_steps
        )];
        if self.warnings > 0 {
            let s = if self.warnings != 1 { "s" } else { "" };
            parts.push(format!("{} warning{s}", self.warnings));
        }
        if self.reordered > 0 {
            parts.push(format!("{} reordered", self.reordered));
        }
        parts.join(" | ")
    }
}

/// Length of the longest strictly increasing subsequence (O(n log n) patience sort).
fn longest_increasing_subsequence_length(seq: &[usize]) -> usize {
    if seq.is_empty() {
        return 0;
    }
    let mut tails: Vec<usize> = Vec::new();
    for &val in seq {
        match tails.binary_search(&val) {
            Ok(_) => {} // duplicate, don't extend
            Err(pos) => {
                if pos == tails.len() {
                    tails.push(val);
                } else {
                    tails[pos] = val;
                }
            }
        }
    }
    tails.len()
}

/// A step validation result (minimal interface to avoid circular dependency).
pub struct StepValidation {
    pub step: StepComment,
    pub result: MatchResult,
}

/// Compute coverage of an algorithm from step validations.
pub fn compute_coverage(
    validations: &[StepValidation],
    algo_steps: &[AlgorithmStep],
    anchor: &str,
) -> CoverageResult {
    let flat = flatten_steps(algo_steps);
    let total = flat.len();

    // Build lookup: step number tuple -> flat index
    let mut step_to_idx = std::collections::HashMap::new();
    let mut all_numbers = std::collections::HashSet::new();
    for (i, s) in flat.iter().enumerate() {
        step_to_idx.insert(s.number.clone(), i);
        all_numbers.insert(s.number.clone());
    }

    let mut implemented: Vec<Vec<u32>> = Vec::new();
    let mut implemented_set = std::collections::HashSet::new();
    let mut spec_order_indices: Vec<usize> = Vec::new();
    let mut warnings = 0;

    for v in validations {
        let key = v.step.number.clone();
        match v.result {
            MatchResult::Exact | MatchResult::Fuzzy => {
                if !implemented_set.contains(&key) {
                    implemented.push(key.clone());
                    implemented_set.insert(key.clone());
                    if let Some(&idx) = step_to_idx.get(&key) {
                        spec_order_indices.push(idx);
                    }
                }
            }
            MatchResult::Mismatch => {
                if !implemented_set.contains(&key) {
                    implemented.push(key.clone());
                    implemented_set.insert(key.clone());
                    if let Some(&idx) = step_to_idx.get(&key) {
                        spec_order_indices.push(idx);
                    }
                }
                warnings += 1;
            }
            MatchResult::NotFound => {
                warnings += 1;
            }
        }
    }

    let missing: Vec<Vec<u32>> = flat
        .iter()
        .filter(|s| !implemented_set.contains(&s.number))
        .map(|s| s.number.clone())
        .collect();

    let lis_len = longest_increasing_subsequence_length(&spec_order_indices);
    let reordered = spec_order_indices.len().saturating_sub(lis_len);

    CoverageResult {
        anchor: anchor.to_string(),
        total_steps: total,
        implemented,
        missing,
        warnings,
        reordered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::steps::parse_steps;

    const SIMPLE_ALGO: &str = "1. First.\n2. Second.\n3. Third.";
    const NESTED_ALGO: &str = "1. Parent.\n\n    1. Child one.\n    2. Child two.\n2. Other.\n";

    fn fake_validation(number: Vec<u32>, result: MatchResult) -> StepValidation {
        StepValidation {
            step: StepComment {
                line: 0,
                col_start: 0,
                col_end: 10,
                number,
                text: String::new(),
                end_line: None,
            },
            result,
        }
    }

    // ── LIS tests ──

    #[test]
    fn lis_empty() {
        assert_eq!(longest_increasing_subsequence_length(&[]), 0);
    }

    #[test]
    fn lis_single() {
        assert_eq!(longest_increasing_subsequence_length(&[5]), 1);
    }

    #[test]
    fn lis_sorted() {
        assert_eq!(longest_increasing_subsequence_length(&[1, 2, 3, 4, 5]), 5);
    }

    #[test]
    fn lis_reverse() {
        assert_eq!(longest_increasing_subsequence_length(&[5, 4, 3, 2, 1]), 1);
    }

    #[test]
    fn lis_mixed() {
        assert_eq!(longest_increasing_subsequence_length(&[1, 3, 2, 5]), 3);
    }

    #[test]
    fn lis_duplicates() {
        assert_eq!(longest_increasing_subsequence_length(&[1, 1, 1]), 1);
    }

    #[test]
    fn lis_longer_sequence() {
        assert_eq!(
            longest_increasing_subsequence_length(&[3, 1, 4, 1, 5, 9, 2, 6]),
            4
        );
    }

    // ── compute_coverage tests ──

    #[test]
    fn all_exact() {
        let steps = parse_steps(SIMPLE_ALGO);
        let vals = vec![
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![2], MatchResult::Exact),
            fake_validation(vec![3], MatchResult::Exact),
        ];
        let cov = compute_coverage(&vals, &steps, "test");
        assert_eq!(cov.total_steps, 3);
        assert_eq!(cov.implemented_count(), 3);
        assert!(cov.missing.is_empty());
        assert_eq!(cov.warnings, 0);
        assert_eq!(cov.reordered, 0);
    }

    #[test]
    fn partial_coverage() {
        let steps = parse_steps(SIMPLE_ALGO);
        let vals = vec![
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![3], MatchResult::Fuzzy),
        ];
        let cov = compute_coverage(&vals, &steps, "test");
        assert_eq!(cov.total_steps, 3);
        assert_eq!(cov.implemented_count(), 2);
        assert_eq!(cov.missing, vec![vec![2u32]]);
        assert_eq!(cov.warnings, 0);
    }

    #[test]
    fn mismatch_counts_as_implemented_with_warning() {
        let steps = parse_steps(SIMPLE_ALGO);
        let vals = vec![
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![2], MatchResult::Mismatch),
        ];
        let cov = compute_coverage(&vals, &steps, "test");
        assert_eq!(cov.implemented_count(), 2);
        assert_eq!(cov.warnings, 1);
        assert_eq!(cov.missing, vec![vec![3u32]]);
    }

    #[test]
    fn not_found_is_warning_only() {
        let steps = parse_steps(SIMPLE_ALGO);
        let vals = vec![
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![99], MatchResult::NotFound),
        ];
        let cov = compute_coverage(&vals, &steps, "test");
        assert_eq!(cov.implemented_count(), 1);
        assert_eq!(cov.warnings, 1);
        assert_eq!(cov.missing.len(), 2);
    }

    #[test]
    fn reordered_detection() {
        let steps = parse_steps(SIMPLE_ALGO);
        let vals = vec![
            fake_validation(vec![3], MatchResult::Exact),
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![2], MatchResult::Exact),
        ];
        let cov = compute_coverage(&vals, &steps, "test");
        assert_eq!(cov.implemented_count(), 3);
        assert_eq!(cov.reordered, 1);
    }

    #[test]
    fn no_validations() {
        let steps = parse_steps(SIMPLE_ALGO);
        let cov = compute_coverage(&[], &steps, "test");
        assert_eq!(cov.total_steps, 3);
        assert_eq!(cov.implemented_count(), 0);
        assert_eq!(cov.missing.len(), 3);
        assert_eq!(cov.warnings, 0);
        assert_eq!(cov.reordered, 0);
    }

    #[test]
    fn nested_coverage() {
        let steps = parse_steps(NESTED_ALGO);
        let vals = vec![
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![1, 2], MatchResult::Fuzzy),
        ];
        let cov = compute_coverage(&vals, &steps, "test");
        assert_eq!(cov.total_steps, 4);
        assert_eq!(cov.implemented_count(), 2);
        assert!(cov.missing.contains(&vec![1, 1]));
        assert!(cov.missing.contains(&vec![2]));
    }

    #[test]
    fn duplicate_step_counted_once() {
        let steps = parse_steps(SIMPLE_ALGO);
        let vals = vec![
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![1], MatchResult::Exact),
            fake_validation(vec![2], MatchResult::Exact),
        ];
        let cov = compute_coverage(&vals, &steps, "test");
        assert_eq!(cov.implemented_count(), 2);
        assert_eq!(cov.missing, vec![vec![3u32]]);
    }

    // ── CoverageResult summary tests ──

    #[test]
    fn summary_all_good() {
        let cov = CoverageResult {
            anchor: "navigate".into(),
            total_steps: 23,
            implemented: (1..=23).map(|i| vec![i]).collect(),
            missing: vec![],
            warnings: 0,
            reordered: 0,
        };
        assert_eq!(cov.summary(), "navigate: 23/23 steps");
    }

    #[test]
    fn summary_with_warnings() {
        let cov = CoverageResult {
            anchor: "navigate".into(),
            total_steps: 23,
            implemented: vec![vec![1], vec![2], vec![3]],
            missing: (4..=23).map(|i| vec![i]).collect(),
            warnings: 2,
            reordered: 0,
        };
        assert_eq!(cov.summary(), "navigate: 3/23 steps | 2 warnings");
    }

    #[test]
    fn summary_with_reordered() {
        let cov = CoverageResult {
            anchor: "navigate".into(),
            total_steps: 10,
            implemented: vec![vec![1], vec![2], vec![3]],
            missing: vec![],
            warnings: 0,
            reordered: 1,
        };
        assert_eq!(cov.summary(), "navigate: 3/10 steps | 1 reordered");
    }

    #[test]
    fn summary_with_all() {
        let cov = CoverageResult {
            anchor: "navigate".into(),
            total_steps: 23,
            implemented: vec![vec![1], vec![2]],
            missing: vec![],
            warnings: 1,
            reordered: 2,
        };
        assert_eq!(
            cov.summary(),
            "navigate: 2/23 steps | 1 warning | 2 reordered"
        );
    }

    #[test]
    fn summary_singular_warning() {
        let cov = CoverageResult {
            anchor: "test".into(),
            total_steps: 5,
            implemented: vec![vec![1]],
            missing: vec![],
            warnings: 1,
            reordered: 0,
        };
        let s = cov.summary();
        assert!(s.contains("1 warning"));
        assert!(!s.contains("warnings"));
    }
}
