//! Document scanning for spec URLs and step comments.

use regex::Regex;

/// A spec URL found in a document.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UrlMatch {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub spec: String,
    pub anchor: String,
    pub url: String,
}

/// A step comment found in source code.
#[derive(Debug, Clone)]
pub struct StepComment {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub number: Vec<u32>,
    pub text: String,
    /// Last line for multi-line comments (None = same as `line`)
    pub end_line: Option<usize>,
}

/// Build a regex from known spec base URLs.
///
/// Matches both single-page URLs (base/#anchor) and multipage URLs
/// (base/multipage/page.html#anchor).
pub fn build_url_pattern(spec_urls: &[SpecUrl]) -> Regex {
    let bases: Vec<String> = spec_urls
        .iter()
        .map(|s| regex::escape(&s.base_url))
        .collect();
    let pattern = format!(r"({})/(?:[^\s#]*)?#([\w:._%{{}}\(\)-]+)", bases.join("|"));
    Regex::new(&pattern).expect("invalid URL pattern")
}

/// Spec name + base URL pair.
#[derive(Debug, Clone)]
pub struct SpecUrl {
    pub spec: String,
    pub base_url: String,
}

/// Build base_url -> spec name lookup.
pub fn build_spec_lookup(spec_urls: &[SpecUrl]) -> std::collections::HashMap<String, String> {
    spec_urls
        .iter()
        .map(|s| (s.base_url.clone(), s.spec.clone()))
        .collect()
}

/// Scan document text for spec URLs.
///
/// Returns list of `UrlMatch` sorted by (line, col_start).
pub fn scan_document(
    text: &str,
    pattern: &Regex,
    spec_lookup: &std::collections::HashMap<String, String>,
) -> Vec<UrlMatch> {
    let mut matches = Vec::new();
    for (line_num, line) in text.lines().enumerate() {
        for m in pattern.find_iter(line) {
            // Re-run with captures to get groups
            if let Some(caps) = pattern.captures(&line[m.start()..]) {
                let base_url = caps.get(1).map_or("", |m| m.as_str());
                let anchor = caps.get(2).map_or("", |m| m.as_str());
                let spec = spec_lookup.get(base_url).cloned().unwrap_or_default();
                matches.push(UrlMatch {
                    line: line_num,
                    col_start: m.start(),
                    col_end: m.end(),
                    spec,
                    anchor: anchor.to_string(),
                    url: m.as_str().to_string(),
                });
            }
        }
    }
    matches
}

/// Step comment pattern matching various comment styles.
///
/// Requires at least one of: "Step" prefix, multi-part number (5.1), trailing dot.
fn step_pattern() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?://|#|;+|/\*+|\*)\s*([Ss]tep\s+)?(\d{1,3}(?:\.\d{1,3})*)(\.)?(?:\s*(.*?))\s*(?:\*/)?$",
        )
        .expect("invalid step pattern")
    })
}

/// Continuation line pattern.
fn continuation_pattern() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\s*(?://|#|;+|\*)\s*(.*?)\s*(?:\*/)?$").expect("invalid continuation pattern")
    })
}

/// Scan document text for step comments.
///
/// Supports multi-line comments: continuation lines immediately following
/// a step comment are appended to its text.
pub fn scan_steps(text: &str) -> Vec<StepComment> {
    let step_re = step_pattern();
    let cont_re = continuation_pattern();
    let lines: Vec<&str> = text.lines().collect();
    let mut results = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if let Some(caps) = step_re.captures(lines[i]) {
            let has_step_prefix = caps.get(1).is_some();
            let number_str = caps.get(2).map_or("", |m| m.as_str());
            let has_trailing_dot = caps.get(3).is_some();
            let mut step_text = caps.get(4).map_or("", |m| m.as_str()).to_string();
            let is_multi_part = number_str.contains('.');

            // Require at least one signal that this is a step reference
            if !has_step_prefix && !has_trailing_dot && !is_multi_part {
                i += 1;
                continue;
            }

            let col_start = caps.get(0).map_or(0, |m| m.start());
            let mut col_end = caps.get(0).map_or(0, |m| m.end());

            // Collect continuation lines
            let mut j = i + 1;
            while j < lines.len() {
                // Stop if the next line is itself a step
                if step_re.is_match(lines[j]) {
                    break;
                }
                if let Some(cont_caps) = cont_re.captures(lines[j]) {
                    let cont_text = cont_caps.get(1).map_or("", |m| m.as_str());
                    if !cont_text.is_empty() {
                        step_text.push(' ');
                        step_text.push_str(cont_text);
                        col_end = cont_caps.get(0).map_or(col_end, |m| m.end());
                        j += 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            let end_line = if j > i + 1 { Some(j - 1) } else { None };
            let number: Vec<u32> = number_str
                .split('.')
                .filter_map(|p| p.parse().ok())
                .collect();

            results.push(StepComment {
                line: i,
                col_start,
                col_end,
                number,
                text: step_text,
                end_line,
            });
            i = j;
        } else {
            i += 1;
        }
    }
    results
}

/// Find a URL match at the given cursor position.
pub fn find_url_at_position(matches: &[UrlMatch], line: usize, col: usize) -> Option<&UrlMatch> {
    matches
        .iter()
        .find(|m| m.line == line && m.col_start <= col && col <= m.col_end)
}

/// Associate step comments with their nearest preceding spec URL.
///
/// A spec URL opens a scope that extends until the next spec URL or EOF.
pub fn build_scopes(
    url_matches: &[UrlMatch],
    step_comments: &[StepComment],
) -> Vec<(UrlMatch, Vec<StepComment>)> {
    if url_matches.is_empty() {
        return Vec::new();
    }

    let mut sorted_urls: Vec<&UrlMatch> = url_matches.iter().collect();
    sorted_urls.sort_by_key(|u| u.line);

    let mut sorted_steps: Vec<&StepComment> = step_comments.iter().collect();
    sorted_steps.sort_by_key(|s| s.line);

    let mut scopes: Vec<(UrlMatch, Vec<StepComment>)> = sorted_urls
        .iter()
        .map(|u| ((*u).clone(), Vec::new()))
        .collect();

    for step in sorted_steps {
        let mut best_scope = None;
        for (i, (url, _)) in scopes.iter().enumerate() {
            if url.line <= step.line {
                best_scope = Some(i);
            } else {
                break;
            }
        }
        if let Some(idx) = best_scope {
            scopes[idx].1.push(step.clone());
        }
    }

    scopes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_spec_urls() -> Vec<SpecUrl> {
        vec![
            SpecUrl {
                spec: "HTML".into(),
                base_url: "https://html.spec.whatwg.org".into(),
            },
            SpecUrl {
                spec: "DOM".into(),
                base_url: "https://dom.spec.whatwg.org".into(),
            },
            SpecUrl {
                spec: "URL".into(),
                base_url: "https://url.spec.whatwg.org".into(),
            },
        ]
    }

    fn pattern() -> Regex {
        build_url_pattern(&test_spec_urls())
    }

    fn lookup() -> std::collections::HashMap<String, String> {
        build_spec_lookup(&test_spec_urls())
    }

    // ── URL pattern tests ──

    #[test]
    fn matches_html_url() {
        let p = pattern();
        let caps = p
            .captures("https://html.spec.whatwg.org/#navigate")
            .unwrap();
        assert_eq!(
            caps.get(1).unwrap().as_str(),
            "https://html.spec.whatwg.org"
        );
        assert_eq!(caps.get(2).unwrap().as_str(), "navigate");
    }

    #[test]
    fn matches_dom_url() {
        let p = pattern();
        let caps = p
            .captures("https://dom.spec.whatwg.org/#concept-tree")
            .unwrap();
        assert_eq!(caps.get(2).unwrap().as_str(), "concept-tree");
    }

    #[test]
    fn no_match_unknown_spec() {
        let p = pattern();
        assert!(p.captures("https://example.com/#foo").is_none());
    }

    #[test]
    fn no_match_without_fragment() {
        let p = pattern();
        assert!(p.captures("https://html.spec.whatwg.org/").is_none());
    }

    #[test]
    fn anchor_with_dots() {
        let p = pattern();
        let caps = p
            .captures("https://html.spec.whatwg.org/#dom-element-click")
            .unwrap();
        assert_eq!(caps.get(2).unwrap().as_str(), "dom-element-click");
    }

    #[test]
    fn anchor_with_colons() {
        let p = pattern();
        let caps = p
            .captures("https://html.spec.whatwg.org/#concept-url-parser:percent-encoded-bytes")
            .unwrap();
        assert_eq!(
            caps.get(2).unwrap().as_str(),
            "concept-url-parser:percent-encoded-bytes"
        );
    }

    #[test]
    fn multipage_url() {
        let p = pattern();
        let caps = p
            .captures("https://html.spec.whatwg.org/multipage/browsing-the-web.html#navigate")
            .unwrap();
        assert_eq!(
            caps.get(1).unwrap().as_str(),
            "https://html.spec.whatwg.org"
        );
        assert_eq!(caps.get(2).unwrap().as_str(), "navigate");
    }

    // ── Scan document tests ──

    #[test]
    fn single_url_in_comment() {
        let text = "// https://html.spec.whatwg.org/#navigate";
        let matches = scan_document(text, &pattern(), &lookup());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].spec, "HTML");
        assert_eq!(matches[0].anchor, "navigate");
        assert_eq!(matches[0].line, 0);
    }

    #[test]
    fn multiple_urls() {
        let text = "// https://html.spec.whatwg.org/#navigate\ncode();\n// https://dom.spec.whatwg.org/#concept-tree\n";
        let matches = scan_document(text, &pattern(), &lookup());
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].spec, "HTML");
        assert_eq!(matches[0].line, 0);
        assert_eq!(matches[1].spec, "DOM");
        assert_eq!(matches[1].line, 2);
    }

    #[test]
    fn no_urls() {
        let text = "just some code\nwith no spec urls\n";
        let matches = scan_document(text, &pattern(), &lookup());
        assert!(matches.is_empty());
    }

    // ── Scan steps tests ──

    #[test]
    fn cpp_step_comment() {
        let text = "// Step 5.1. Assert: userInvolvement is browser UI";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![5, 1]);
        assert!(steps[0].text.contains("Assert"));
    }

    #[test]
    fn step_without_prefix() {
        let text = "// 5.1. Let x be something";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![5, 1]);
    }

    #[test]
    fn step_no_trailing_dot() {
        let text = "// Step 5.1 Assert: foo";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![5, 1]);
    }

    #[test]
    fn step_number_only() {
        let text = "// Step 5.";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![5]);
        assert_eq!(steps[0].text, "");
    }

    #[test]
    fn python_step_comment() {
        let text = "# Step 3. Do something";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![3]);
    }

    #[test]
    fn css_step_comment() {
        let text = "/* Step 1. Init */";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![1]);
        assert_eq!(steps[0].text, "Init");
    }

    #[test]
    fn no_step_comment() {
        let text = "// This is just a regular comment";
        let steps = scan_steps(text);
        assert!(steps.is_empty());
    }

    #[test]
    fn multiple_steps() {
        let text = "// Step 1. First\n// Step 2. Second\n// Step 3. Third";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].line, 0);
        assert_eq!(steps[1].line, 1);
        assert_eq!(steps[2].line, 2);
    }

    #[test]
    fn deeply_nested_number() {
        let text = "// Step 5.1.2 Deeply nested step";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![5, 1, 2]);
    }

    #[test]
    fn asm_comment() {
        let text = "; Step 1. Assembly step";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![1]);
    }

    #[test]
    fn bare_number_not_matched() {
        let text = "// 42 is the answer to life";
        let steps = scan_steps(text);
        assert!(steps.is_empty());
    }

    #[test]
    fn bare_number_with_port() {
        let text = "// Use port 8080";
        let steps = scan_steps(text);
        assert!(steps.is_empty());
    }

    #[test]
    fn single_number_with_trailing_dot() {
        let text = "// 5. Let x be something";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![5]);
    }

    #[test]
    fn multi_part_without_prefix_or_dot() {
        let text = "// 5.1 Let x be something";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![5, 1]);
    }

    #[test]
    fn multiline_continuation() {
        let text = "// Step 2.1 Foo Bar baz\n//       continues here";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].number, vec![2, 1]);
        assert_eq!(steps[0].text, "Foo Bar baz continues here");
        assert_eq!(steps[0].line, 0);
    }

    #[test]
    fn multiline_stops_at_next_step() {
        let text = "// Step 1. First\n//   more first\n// Step 2. Second";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].text, "First more first");
        assert_eq!(steps[1].text, "Second");
    }

    #[test]
    fn multiline_stops_at_non_comment() {
        let text = "// Step 1. First\ncode();\n// Step 2. Second";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].text, "First");
        assert_eq!(steps[1].text, "Second");
    }

    // ── find_url_at_position tests ──

    #[test]
    fn cursor_on_url() {
        let text = "// https://html.spec.whatwg.org/#navigate";
        let matches = scan_document(text, &pattern(), &lookup());
        assert!(find_url_at_position(&matches, 0, 10).is_some());
    }

    #[test]
    fn cursor_before_url() {
        let text = "// https://html.spec.whatwg.org/#navigate";
        let matches = scan_document(text, &pattern(), &lookup());
        assert!(find_url_at_position(&matches, 0, 0).is_none());
    }

    #[test]
    fn cursor_wrong_line() {
        let text = "// https://html.spec.whatwg.org/#navigate\nfoo";
        let matches = scan_document(text, &pattern(), &lookup());
        assert!(find_url_at_position(&matches, 1, 0).is_none());
    }

    // ── build_scopes tests ──

    #[test]
    fn single_url_with_steps() {
        let text =
            "// https://html.spec.whatwg.org/#navigate\n// Step 1. First\n// Step 2. Second\n";
        let urls = scan_document(text, &pattern(), &lookup());
        let steps = scan_steps(text);
        let scopes = build_scopes(&urls, &steps);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 2);
    }

    #[test]
    fn two_urls_split_steps() {
        let text = "// https://html.spec.whatwg.org/#navigate\n// Step 1. From navigate\n// https://dom.spec.whatwg.org/#concept-tree\n// Step 1. From tree\n";
        let urls = scan_document(text, &pattern(), &lookup());
        let steps = scan_steps(text);
        let scopes = build_scopes(&urls, &steps);
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 1);
        assert_eq!(scopes[1].0.anchor, "concept-tree");
        assert_eq!(scopes[1].1.len(), 1);
    }

    #[test]
    fn steps_before_any_url() {
        let text = "// Step 1. Orphan step\n// https://html.spec.whatwg.org/#navigate\n// Step 2. Assigned step\n";
        let urls = scan_document(text, &pattern(), &lookup());
        let steps = scan_steps(text);
        let scopes = build_scopes(&urls, &steps);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].1.len(), 1);
        assert_eq!(scopes[0].1[0].number, vec![2]);
    }

    #[test]
    fn no_urls_empty_scopes() {
        let text = "// Step 1. Orphan";
        let urls = scan_document(text, &pattern(), &lookup());
        let steps = scan_steps(text);
        let scopes = build_scopes(&urls, &steps);
        assert!(scopes.is_empty());
    }
}
