//! Algorithm step parsing from spec markdown content.

use regex::Regex;

/// A single step in a spec algorithm.
#[derive(Debug, Clone)]
pub struct AlgorithmStep {
    pub number: Vec<u32>,
    pub text: String,
    pub children: Vec<AlgorithmStep>,
}

/// Strip markdown inline formatting, keeping the text content.
pub fn strip_markdown(text: &str) -> String {
    use std::sync::OnceLock;
    static LINK_RE: OnceLock<Regex> = OnceLock::new();
    static BOLD_RE: OnceLock<Regex> = OnceLock::new();
    static ITALIC_RE: OnceLock<Regex> = OnceLock::new();
    static CODE_RE: OnceLock<Regex> = OnceLock::new();

    let link_re = LINK_RE.get_or_init(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap());
    let bold_re = BOLD_RE.get_or_init(|| Regex::new(r"\*\*([^*]*)\*\*").unwrap());
    let italic_re = ITALIC_RE.get_or_init(|| Regex::new(r"\*([^*]*)\*").unwrap());
    let code_re = CODE_RE.get_or_init(|| Regex::new(r"`([^`]*)`").unwrap());

    let text = link_re.replace_all(text, "$1");
    let text = bold_re.replace_all(&text, "$1");
    let text = italic_re.replace_all(&text, "$1");
    let text = code_re.replace_all(&text, "$1");
    text.to_string()
}

/// Matches a numbered list item: optional indentation, then "N. text"
fn step_line_re() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^( *)\d+\.\s").unwrap())
}

/// Parse a numbered list line.
///
/// Returns (indent_level, step_num, text) or None if not a step line.
fn parse_step_line(line: &str) -> Option<(usize, u32, String)> {
    let re = step_line_re();
    let m = re.find(line)?;
    let spaces = line.len() - line.trim_start().len();
    let indent = spaces / 4;

    let rest = line.trim_start();
    let dot_pos = rest.find('.')?;
    let num: u32 = rest[..dot_pos].parse().ok()?;
    let text = rest[dot_pos + 1..].trim().to_string();
    let _ = m; // used for match detection
    Some((indent, num, text))
}

/// Parse algorithm steps from markdown content.
///
/// Expects the content field from query results, which contains numbered lists
/// at various indentation levels representing algorithm steps.
pub fn parse_steps(content: &str) -> Vec<AlgorithmStep> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut raw_steps: Vec<(usize, u32, String)> = Vec::new(); // (indent, num, text)

    let mut i = 0;
    while i < lines.len() {
        if let Some((indent, num, mut text)) = parse_step_line(lines[i]) {
            // Accumulate continuation lines
            let mut j = i + 1;
            while j < lines.len() {
                let next_line = lines[j];
                if next_line.trim().is_empty() {
                    j += 1;
                    continue;
                }
                if parse_step_line(next_line).is_some() {
                    break;
                }
                let stripped = next_line.trim_start();
                let next_indent = next_line.len() - stripped.len();
                let step_indent = indent * 4;
                if next_indent > step_indent
                    && !stripped.starts_with('>')
                    && !stripped.starts_with('*')
                {
                    text.push(' ');
                    text.push_str(stripped);
                } else {
                    break;
                }
                j += 1;
            }
            raw_steps.push((indent, num, text));
            i = j;
        } else {
            i += 1;
        }
    }

    // Build hierarchical step tree
    let mut steps: Vec<AlgorithmStep> = Vec::new();
    // Stack of (indent_level, &mut children_vec)
    // We use indices to avoid borrow issues
    struct StackEntry {
        indent: isize,
        // Index path to the children vec in the tree
        path: Vec<usize>,
    }
    let mut stack: Vec<StackEntry> = vec![StackEntry {
        indent: -1,
        path: vec![],
    }];

    for (indent, _num, text) in &raw_steps {
        let plain_text = strip_markdown(text);
        let indent = *indent as isize;

        // Pop stack until we find the parent level
        while stack.len() > 1 && stack.last().unwrap().indent >= indent {
            stack.pop();
        }

        let parent_path = stack.last().unwrap().path.clone();

        // Navigate to the parent's children list and add the new step
        let step = AlgorithmStep {
            number: vec![], // assigned later
            text: plain_text,
            children: vec![],
        };

        if parent_path.is_empty() {
            steps.push(step);
            let new_idx = steps.len() - 1;
            let mut child_path = parent_path;
            child_path.push(new_idx);
            stack.push(StackEntry {
                indent,
                path: child_path,
            });
        } else {
            // Navigate to parent step using path
            let children = get_children_mut(&mut steps, &parent_path);
            children.push(step);
            let new_idx = children.len() - 1;
            let mut child_path = parent_path;
            child_path.push(new_idx);
            stack.push(StackEntry {
                indent,
                path: child_path,
            });
        }
    }

    assign_numbers(&mut steps, &[]);
    steps
}

/// Navigate to children of the step at the given path.
fn get_children_mut<'a>(
    root: &'a mut [AlgorithmStep],
    path: &[usize],
) -> &'a mut Vec<AlgorithmStep> {
    if path.is_empty() {
        panic!("empty path in get_children_mut");
    }
    let mut current = &mut root[path[0]];
    for &idx in &path[1..] {
        current = &mut current.children[idx];
    }
    &mut current.children
}

/// Assign hierarchical step numbers based on tree position.
fn assign_numbers(steps: &mut [AlgorithmStep], prefix: &[u32]) {
    for (i, step) in steps.iter_mut().enumerate() {
        let mut num = prefix.to_vec();
        num.push((i + 1) as u32);
        step.number = num.clone();
        assign_numbers(&mut step.children, &num);
    }
}

/// Find a step by its hierarchical number path.
pub fn find_step<'a>(steps: &'a [AlgorithmStep], number: &[u32]) -> Option<&'a AlgorithmStep> {
    if number.is_empty() {
        return None;
    }
    let mut current = steps;
    let mut target = None;
    for &n in number {
        if n < 1 || n as usize > current.len() {
            return None;
        }
        target = Some(&current[(n - 1) as usize]);
        current = &target.unwrap().children;
    }
    target
}

/// Flatten a step tree into a list (depth-first).
pub fn flatten_steps(steps: &[AlgorithmStep]) -> Vec<&AlgorithmStep> {
    let mut result = Vec::new();
    for step in steps {
        result.push(step);
        result.extend(flatten_steps(&step.children));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_bold() {
        assert_eq!(strip_markdown("**bold**"), "bold");
    }

    #[test]
    fn strip_italic() {
        assert_eq!(strip_markdown("*italic*"), "italic");
    }

    #[test]
    fn strip_code() {
        assert_eq!(strip_markdown("`code`"), "code");
    }

    #[test]
    fn strip_link() {
        assert_eq!(strip_markdown("[text](https://example.com)"), "text");
    }

    #[test]
    fn strip_mixed() {
        let result = strip_markdown("Let *x* be the result of [foo](https://bar.com)");
        assert_eq!(result, "Let x be the result of foo");
    }

    #[test]
    fn strip_nested_bold_link() {
        let result = strip_markdown("[**bold link**](url)");
        assert_eq!(result, "bold link");
    }

    #[test]
    fn simple_flat() {
        let content = "1. First step.\n2. Second step.\n3. Third step.";
        let steps = parse_steps(content);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].number, vec![1]);
        assert_eq!(steps[1].number, vec![2]);
        assert_eq!(steps[2].number, vec![3]);
        assert!(steps[0].text.contains("First step"));
        assert!(steps[1].text.contains("Second step"));
    }

    #[test]
    fn nested_steps() {
        let content = "1. Parent step.\n\n    1. Child one.\n    2. Child two.\n2. Next parent.\n";
        let steps = parse_steps(content);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].number, vec![1]);
        assert_eq!(steps[1].number, vec![2]);
        assert_eq!(steps[0].children.len(), 2);
        assert_eq!(steps[0].children[0].number, vec![1, 1]);
        assert_eq!(steps[0].children[1].number, vec![1, 2]);
    }

    #[test]
    fn deeply_nested() {
        let content = "1. Top level.\n\n    1. Second level.\n\n        1. Third level.\n        2. Third level b.\n    2. Second level b.\n2. Top level b.\n";
        let steps = parse_steps(content);
        assert_eq!(steps.len(), 2);
        let deep = &steps[0].children[0].children[0];
        assert_eq!(deep.number, vec![1, 1, 1]);
        assert_eq!(steps[0].children[0].children[1].number, vec![1, 1, 2]);
    }

    #[test]
    fn preamble_ignored() {
        let content = "To **navigate** a navigable:\n\n1. First actual step.\n2. Second step.\n";
        let steps = parse_steps(content);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].number, vec![1]);
    }

    #[test]
    fn markdown_stripped_from_text() {
        let content = "1. Let *cspNavigationType* be \"`form-submission`\".";
        let steps = parse_steps(content);
        assert_eq!(steps.len(), 1);
        assert!(steps[0].text.contains("cspNavigationType"));
        assert!(!steps[0].text.contains('*'));
    }

    #[test]
    fn empty_content() {
        assert!(parse_steps("").is_empty());
    }

    #[test]
    fn no_steps() {
        assert!(parse_steps("Just a paragraph with no numbered list.").is_empty());
    }

    #[test]
    fn find_top_level() {
        let steps = parse_steps("1. A.\n2. B.\n3. C.");
        assert_eq!(find_step(&steps, &[2]).unwrap().text, "B.");
    }

    #[test]
    fn find_nested() {
        let content = "1. Parent.\n\n    1. Child.\n    2. Child b.\n2. Other.";
        let steps = parse_steps(content);
        let step = find_step(&steps, &[1, 2]);
        assert!(step.is_some());
        assert!(step.unwrap().text.contains("Child b"));
    }

    #[test]
    fn find_not_found() {
        let steps = parse_steps("1. A.\n2. B.");
        assert!(find_step(&steps, &[99]).is_none());
    }

    #[test]
    fn find_empty_number() {
        let steps = parse_steps("1. A.");
        assert!(find_step(&steps, &[]).is_none());
    }

    #[test]
    fn flatten_flat() {
        let steps = parse_steps("1. A.\n2. B.\n3. C.");
        let flat = flatten_steps(&steps);
        assert_eq!(flat.len(), 3);
        assert_eq!(flat[0].number, vec![1]);
        assert_eq!(flat[1].number, vec![2]);
        assert_eq!(flat[2].number, vec![3]);
    }

    #[test]
    fn flatten_nested() {
        let content = "1. Parent.\n\n    1. Child.\n    2. Child b.\n2. Other.";
        let steps = parse_steps(content);
        let flat = flatten_steps(&steps);
        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0].number, vec![1]);
        assert_eq!(flat[1].number, vec![1, 1]);
        assert_eq!(flat[2].number, vec![1, 2]);
        assert_eq!(flat[3].number, vec![2]);
    }
}
