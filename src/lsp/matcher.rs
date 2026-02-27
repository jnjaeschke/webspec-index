//! Fuzzy text matching for step validation.

use regex::Regex;
use strsim::jaro_winkler;

use super::steps::strip_markdown;

/// Result of matching a step comment against the spec text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Exact,
    Fuzzy,
    Mismatch,
    NotFound,
}

impl MatchResult {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchResult::Exact => "exact",
            MatchResult::Fuzzy => "fuzzy",
            MatchResult::Mismatch => "mismatch",
            MatchResult::NotFound => "not_found",
        }
    }
}

/// Normalize text for comparison.
///
/// Strips markdown, collapses whitespace, lowercases, strips trailing punctuation.
pub fn normalize_text(text: &str) -> String {
    fn whitespace_re() -> &'static Regex {
        use std::sync::OnceLock;
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"\s+").unwrap())
    }
    fn trailing_punct_re() -> &'static Regex {
        use std::sync::OnceLock;
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new(r"[.,:;!?]+$").unwrap())
    }

    let text = strip_markdown(text);
    let text = whitespace_re().replace_all(&text, " ");
    let text = text.trim().to_lowercase();
    let text = trailing_punct_re().replace(&text, "");
    text.to_string()
}

/// Classify how well a step comment matches the spec text.
pub fn classify_match(comment_text: &str, spec_text: &str, threshold: f64) -> MatchResult {
    if comment_text.trim().is_empty() {
        // Step number only, no text to compare — counts as exact
        return MatchResult::Exact;
    }

    let norm_comment = normalize_text(comment_text);
    let norm_spec = normalize_text(spec_text);

    if norm_comment.is_empty() {
        return MatchResult::Exact;
    }
    if norm_spec.is_empty() {
        return MatchResult::Mismatch;
    }

    if norm_comment == norm_spec {
        return MatchResult::Exact;
    }

    // Prefix/substring match
    if norm_spec.starts_with(&norm_comment) || norm_comment.starts_with(&norm_spec) {
        return MatchResult::Fuzzy;
    }

    if norm_comment.contains(&norm_spec) || norm_spec.contains(&norm_comment) {
        return MatchResult::Fuzzy;
    }

    let similarity = jaro_winkler(&norm_comment, &norm_spec);
    if similarity >= threshold {
        return MatchResult::Fuzzy;
    }

    MatchResult::Mismatch
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize_text tests ──

    #[test]
    fn strips_markdown() {
        assert_eq!(normalize_text("Let *x* be **y**"), "let x be y");
    }

    #[test]
    fn strips_code() {
        assert_eq!(
            normalize_text("the \"`form-submission`\" type"),
            "the \"form-submission\" type"
        );
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(normalize_text("foo   bar\tbaz"), "foo bar baz");
    }

    #[test]
    fn lowercases() {
        assert_eq!(
            normalize_text("Assert: userInvolvement"),
            "assert: userinvolvement"
        );
    }

    #[test]
    fn strips_trailing_punct() {
        assert_eq!(normalize_text("some text."), "some text");
        assert_eq!(normalize_text("some text..."), "some text");
        assert_eq!(normalize_text("some text;"), "some text");
    }

    #[test]
    fn strips_links() {
        assert_eq!(
            normalize_text("[Assert](https://example.com): foo"),
            "assert: foo"
        );
    }

    #[test]
    fn empty_string() {
        assert_eq!(normalize_text(""), "");
    }

    // ── jaro_winkler tests (via strsim) ──

    #[test]
    fn jw_identical() {
        assert_eq!(jaro_winkler("hello", "hello"), 1.0);
    }

    #[test]
    fn jw_empty_strings() {
        assert_eq!(jaro_winkler("", ""), 1.0);
        assert_eq!(jaro_winkler("hello", ""), 0.0);
        assert_eq!(jaro_winkler("", "hello"), 0.0);
    }

    #[test]
    fn jw_similar() {
        let score = jaro_winkler("martha", "marhta");
        assert!(score > 0.9);
    }

    #[test]
    fn jw_different() {
        let score = jaro_winkler("hello", "world");
        assert!(score < 0.5);
    }

    #[test]
    fn jw_prefix_boost() {
        let score1 = jaro_winkler("navigation", "navigating");
        let score2 = jaro_winkler("navigation", "xavigation");
        assert!(score1 > score2);
    }

    // ── classify_match tests ──

    #[test]
    fn exact_match() {
        let result = classify_match(
            "Let cspNavigationType be form-submission",
            "Let *cspNavigationType* be `form-submission`",
            0.85,
        );
        assert_eq!(result, MatchResult::Exact);
    }

    #[test]
    fn empty_comment_text() {
        let result = classify_match("", "Some spec text", 0.85);
        assert_eq!(result, MatchResult::Exact);
    }

    #[test]
    fn prefix_match() {
        let result = classify_match(
            "Let cspNavigationType be",
            "Let *cspNavigationType* be \"`form-submission`\" if *formDataEntryList* is non-null",
            0.85,
        );
        assert_eq!(result, MatchResult::Fuzzy);
    }

    #[test]
    fn mismatch() {
        let result = classify_match(
            "Do something completely different",
            "Let x be the result of running foo",
            0.85,
        );
        assert_eq!(result, MatchResult::Mismatch);
    }

    #[test]
    fn both_empty() {
        assert_eq!(classify_match("", "", 0.85), MatchResult::Exact);
    }

    #[test]
    fn comment_only_whitespace() {
        assert_eq!(classify_match("   ", "Some text", 0.85), MatchResult::Exact);
    }
}
