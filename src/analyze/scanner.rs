//! Document scanning for spec URLs and step comments.

use regex::Regex;

/// A spec URL found in a document.
#[derive(Debug, Clone)]
pub struct UrlMatch {
    pub line: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub indent: usize,
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
    pub indent: usize,
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

/// Count leading whitespace characters on a line.
fn leading_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
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
        let indent = leading_indent(line);
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
                    indent,
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

            let indent = leading_indent(lines[i]);
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
                indent,
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

/// Associate step comments with spec URLs using indentation-based scoping.
///
/// Scoping rules:
/// - A spec URL comment at indent level N opens a scope.
/// - A URL at the same indent as the top of the scope stack replaces it;
///   a URL at deeper indent stacks on top (nested scope).
/// - Step comments are assigned to the innermost (top-of-stack) scope.
/// - Scopes close when a non-blank line at indent L satisfies:
///   - `L < N` (left the block entirely), OR
///   - `L == N` and the scope saw deeper content (`max_seen > N`) —
///     this catches closing braces returning to the scope's indent level.
///
/// This correctly handles:
/// - Comments above a function (scope survives the function signature, closes at `}`)
/// - Comments inside a function body (scope closes at `}` which is at lower indent)
/// - Nested spec URLs (inner algorithm inside an outer one)
pub fn build_scopes(
    text: &str,
    url_matches: &[UrlMatch],
    step_comments: &[StepComment],
) -> Vec<(UrlMatch, Vec<StepComment>)> {
    if url_matches.is_empty() {
        return Vec::new();
    }

    // Index url_matches and step_comments by line number for O(1) lookup.
    let mut url_by_line: std::collections::HashMap<usize, Vec<&UrlMatch>> =
        std::collections::HashMap::new();
    for u in url_matches {
        url_by_line.entry(u.line).or_default().push(u);
    }

    let mut step_by_line: std::collections::HashMap<usize, Vec<&StepComment>> =
        std::collections::HashMap::new();
    for s in step_comments {
        step_by_line.entry(s.line).or_default().push(s);
    }

    // Scope stack: each entry tracks the URL, its indent, the maximum indent seen
    // since it was pushed, and the collected step comments.
    struct Scope {
        url: UrlMatch,
        indent: usize,
        max_seen: usize,
        steps: Vec<StepComment>,
    }

    let mut stack: Vec<Scope> = Vec::new();
    let mut finished: Vec<(UrlMatch, Vec<StepComment>)> = Vec::new();

    let lines: Vec<&str> = text.lines().collect();

    for (line_num, line_text) in lines.iter().enumerate() {
        let indent = leading_indent(line_text);
        let is_blank = line_text.trim().is_empty();

        // Blank lines don't affect scoping.
        if is_blank {
            continue;
        }

        // Check if this line has URL matches — handle scope push/replace.
        if let Some(urls) = url_by_line.get(&line_num) {
            for url in urls {
                let url_indent = url.indent;

                // If the top of stack has the same indent, replace it (same block,
                // different algorithm). Otherwise stack (nested scope).
                if let Some(top) = stack.last() {
                    if url_indent <= top.indent {
                        // Pop scopes at same or higher indent before pushing.
                        while let Some(top) = stack.last() {
                            if top.indent >= url_indent {
                                let popped = stack.pop().unwrap();
                                finished.push((popped.url, popped.steps));
                            } else {
                                break;
                            }
                        }
                    }
                }

                stack.push(Scope {
                    url: (*url).clone(),
                    indent: url_indent,
                    max_seen: url_indent,
                    steps: Vec::new(),
                });
            }
            continue;
        }

        // Check if this line has step comments — assign to top of stack.
        if let Some(steps) = step_by_line.get(&line_num) {
            if let Some(top) = stack.last_mut() {
                if indent > top.max_seen {
                    top.max_seen = indent;
                }
                for step in steps {
                    top.steps.push((*step).clone());
                }
            }
            // Step comment lines don't close scopes (they're comments).
            continue;
        }

        // Regular line: update max_seen and check scope closing.
        // Close scopes from top of stack where the closing condition is met.
        while let Some(top) = stack.last() {
            let should_close =
                indent < top.indent || (indent == top.indent && top.max_seen > top.indent);
            if should_close {
                let popped = stack.pop().unwrap();
                finished.push((popped.url, popped.steps));
            } else {
                break;
            }
        }

        // Update max_seen for remaining top scope.
        if let Some(top) = stack.last_mut() {
            if indent > top.max_seen {
                top.max_seen = indent;
            }
        }
    }

    // Flush remaining scopes (EOF closes everything).
    while let Some(scope) = stack.pop() {
        finished.push((scope.url, scope.steps));
    }

    // Sort by the URL's line number so output order is deterministic.
    finished.sort_by_key(|(url, _)| url.line);
    finished
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

    /// Helper: scan text and build scopes in one call.
    fn scopes_for(text: &str) -> Vec<(UrlMatch, Vec<StepComment>)> {
        let urls = scan_document(text, &pattern(), &lookup());
        let steps = scan_steps(text);
        build_scopes(text, &urls, &steps)
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
        assert_eq!(matches[0].indent, 0);
    }

    #[test]
    fn indented_url() {
        let text = "    // https://html.spec.whatwg.org/#navigate";
        let matches = scan_document(text, &pattern(), &lookup());
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].indent, 4);
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
        assert_eq!(steps[0].indent, 0);
    }

    #[test]
    fn indented_step() {
        let text = "      // Step 1. Do something";
        let steps = scan_steps(text);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].indent, 6);
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

    // ── build_scopes tests (indentation-based) ──

    #[test]
    fn scope_simple_flat() {
        // All at indent 0: URL + steps, no closing brace.
        let text = "\
// https://html.spec.whatwg.org/#navigate
// Step 1. First
// Step 2. Second
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 2);
    }

    #[test]
    fn scope_comment_above_function() {
        // URL at indent 0, function body at indent 4, closing } at indent 0.
        let text = "\
// https://html.spec.whatwg.org/#navigate
void DoNavigate() {
    // Step 1. First
    code();
    // Step 2. Second
    more_code();
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 2);
    }

    #[test]
    fn scope_comment_inside_function() {
        // URL at indent 4 (inside function body), } at indent 0 closes it.
        let text = "\
void DoNavigate() {
    // https://html.spec.whatwg.org/#navigate
    // Step 1. First
    code();
    // Step 2. Second
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 2);
    }

    #[test]
    fn scope_class_member_closes_at_brace() {
        // URL at indent 2 (class member), function body at indent 4, } at indent 2 closes scope.
        let text = "\
class Foo {
  // https://html.spec.whatwg.org/#navigate
  void foo() {
    // Step 1. Do this
    do_this();
    // Step 2. Do that
    do_that();
  }

  void bar() {
    // Step 3. Should not be in navigate scope
    other();
  }
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 2);
        assert_eq!(scopes[0].1[0].number, vec![1]);
        assert_eq!(scopes[0].1[1].number, vec![2]);
    }

    #[test]
    fn scope_two_separate_functions() {
        // Two functions, each with its own spec URL.
        let text = "\
class Foo {
  // https://html.spec.whatwg.org/#navigate
  void navigate() {
    // Step 1. Nav step
    nav();
  }

  // https://dom.spec.whatwg.org/#concept-tree
  void tree() {
    // Step 1. Tree step
    tree_op();
  }
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 1);
        assert_eq!(scopes[0].1[0].number, vec![1]);
        assert_eq!(scopes[1].0.anchor, "concept-tree");
        assert_eq!(scopes[1].1.len(), 1);
        assert_eq!(scopes[1].1[0].number, vec![1]);
    }

    #[test]
    fn scope_nested_stacked() {
        // Outer algorithm with an inner algorithm nested inside.
        let text = "\
void Navigate() {
    // https://html.spec.whatwg.org/#navigate
    // Step 1. Outer step one
    code();
    if (cond) {
        // https://dom.spec.whatwg.org/#concept-tree
        // Step 1. Inner step one
        inner_code();
    }
    // Step 2. Outer step two
    more_code();
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 2);
        // Inner scope
        assert_eq!(scopes[1].0.anchor, "concept-tree");
        assert_eq!(scopes[1].1.len(), 1);
        assert_eq!(scopes[1].1[0].number, vec![1]);
        // Outer scope
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 2);
        assert_eq!(scopes[0].1[0].number, vec![1]);
        assert_eq!(scopes[0].1[1].number, vec![2]);
    }

    #[test]
    fn scope_same_indent_replaces() {
        // Two URLs at the same indent level replace each other.
        let text = "\
void foo() {
    // https://html.spec.whatwg.org/#navigate
    // Step 1. Navigate step
    code();

    // https://dom.spec.whatwg.org/#concept-tree
    // Step 1. Tree step
    more_code();
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 1);
        assert_eq!(scopes[0].1[0].text, "Navigate step");
        assert_eq!(scopes[1].0.anchor, "concept-tree");
        assert_eq!(scopes[1].1.len(), 1);
        assert_eq!(scopes[1].1[0].text, "Tree step");
    }

    #[test]
    fn scope_orphan_steps_ignored() {
        // Steps before any URL are not assigned to any scope.
        let text = "\
// Step 1. Orphan step
// https://html.spec.whatwg.org/#navigate
// Step 2. Assigned step
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].1.len(), 1);
        assert_eq!(scopes[0].1[0].number, vec![2]);
    }

    #[test]
    fn scope_no_urls_empty() {
        let text = "// Step 1. Orphan";
        let scopes = scopes_for(text);
        assert!(scopes.is_empty());
    }

    #[test]
    fn scope_deeply_nested_stack() {
        // Three levels of nesting.
        let text = "\
class Outer {
  // https://html.spec.whatwg.org/#navigate
  void foo() {
    // Step 1. Outer step
    if (a) {
      // https://dom.spec.whatwg.org/#concept-tree
      // Step 1. Middle step
      if (b) {
        // https://url.spec.whatwg.org/#url-parsing
        // Step 1. Inner step
        parse();
      }
      // Step 2. Middle step two
      tree();
    }
    // Step 2. Outer step two
    done();
  }
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 3);

        let nav = scopes.iter().find(|(u, _)| u.anchor == "navigate").unwrap();
        assert_eq!(nav.1.len(), 2);
        assert_eq!(nav.1[0].number, vec![1]);
        assert_eq!(nav.1[1].number, vec![2]);

        let tree = scopes
            .iter()
            .find(|(u, _)| u.anchor == "concept-tree")
            .unwrap();
        assert_eq!(tree.1.len(), 2);
        assert_eq!(tree.1[0].number, vec![1]);
        assert_eq!(tree.1[1].number, vec![2]);

        let url = scopes
            .iter()
            .find(|(u, _)| u.anchor == "url-parsing")
            .unwrap();
        assert_eq!(url.1.len(), 1);
        assert_eq!(url.1[0].number, vec![1]);
    }

    #[test]
    fn scope_existing_fixture_compat() {
        // Matches the existing test fixture: input.cpp
        let text = "\
// https://html.spec.whatwg.org/#navigate
void DoNavigate(bool userInvolvement) {
  // Step 1. Let cspNavigationType be form-submission
  auto cspNavigationType = GetCSPNavType();

  // Step 2. Let sourceSnapshotParams be the result of snapshotting
  auto params = SnapshotParams();

  // Step 3. If url is about:blank, then return
  if (IsAboutBlank(url)) {
    return;
  }

  // Step 99. Nonexistent step
  DoSomething();
}
";
        let scopes = scopes_for(text);
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].0.anchor, "navigate");
        assert_eq!(scopes[0].1.len(), 4);
        assert_eq!(scopes[0].1[0].number, vec![1]);
        assert_eq!(scopes[0].1[1].number, vec![2]);
        assert_eq!(scopes[0].1[2].number, vec![3]);
        assert_eq!(scopes[0].1[3].number, vec![99]);
    }
}
