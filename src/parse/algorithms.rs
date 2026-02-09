// Algorithm rendering: convert <ol>/<li> to numbered markdown text
use htmd::HtmlToMarkdown;
use scraper::{ElementRef, Node};

/// Render an algorithm's `<ol>` element with markdown-style numbering.
/// Nested lists use simple numbering (1., 2., etc.) with indentation - markdown handles visual hierarchy.
/// Inline content is converted to markdown using the provided converter.
pub fn render_algorithm_ol(ol_element: &ElementRef, converter: &HtmlToMarkdown) -> String {
    let mut result = String::new();
    let mut step_number = 1;

    for child in ol_element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            let tag_name = child_element.value().name();

            if tag_name == "li" {
                let step_text =
                    render_li_recursive(&child_element, &[step_number], 0, converter);
                result.push_str(&step_text);
                step_number += 1;
            } else {
                // Handle other elements between list items (notes, examples, etc.)
                let elem_md = converter
                    .convert(&child_element.html())
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                if !elem_md.is_empty() {
                    result.push_str("\n\n");
                    result.push_str(&elem_md);
                    result.push('\n');
                }
            }
        }
    }

    result.trim_end().to_string()
}

/// Recursively render a `<li>` element with simple numbering (markdown handles hierarchy via indentation)
fn render_li_recursive(
    li: &ElementRef,
    numbering: &[usize],
    indent: usize,
    converter: &HtmlToMarkdown,
) -> String {
    let mut result = String::new();

    // Add indentation (4 spaces per level for markdown list continuation)
    // This makes markdown parsers treat nested items as part of the parent list
    for _ in 0..indent {
        result.push_str("    ");
    }

    // Add step number - just the current level number (markdown auto-numbers based on indentation)
    let step_num = numbering.last().unwrap_or(&1);
    result.push_str(&format!("{}. ", step_num));

    // Process children in document order, preserving structure
    let mut first_content = true;
    for child in li.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            let tag_name = child_element.value().name();

            if tag_name == "ol" {
                // Nested numbered list - add blank line before for markdown recognition
                result.push_str("\n\n");
                let mut sub_step = 1;
                for sub_child in child_element.children() {
                    if let Some(sub_li) = ElementRef::wrap(sub_child) {
                        if sub_li.value().name() == "li" {
                            let mut new_numbering = numbering.to_vec();
                            new_numbering.push(sub_step);
                            result.push_str(&render_li_recursive(
                                &sub_li,
                                &new_numbering,
                                indent + 1,
                                converter,
                            ));
                            sub_step += 1;
                        }
                    }
                }
                first_content = false;
            } else if tag_name == "ul" {
                // Nested bullet list - add blank line before for markdown recognition
                result.push_str("\n\n");
                result.push_str(&render_ul(&child_element, indent + 1, converter));
                first_content = false;
            } else {
                // Regular content (p, div, etc.)
                let elem_md = converter
                    .convert(&child_element.html())
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                if !elem_md.is_empty() {
                    if first_content {
                        result.push_str(&elem_md);
                        first_content = false;
                    } else {
                        // Continuation content needs indentation to stay part of list item
                        result.push_str("\n\n");
                        let indented = indent_lines(&elem_md, indent + 1);
                        result.push_str(&indented);
                    }
                    result.push('\n');
                }
            }
        } else if let Node::Text(text) = child.value() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                result.push_str(trimmed);
                result.push(' ');
            }
        }
    }

    result
}

/// Indent every line of a multi-line string with N levels of 4-space indentation
fn indent_lines(text: &str, indent: usize) -> String {
    let prefix = "    ".repeat(indent);
    text.lines()
        .map(|line| {
            if line.trim().is_empty() {
                line.to_string()
            } else {
                format!("{}{}", prefix, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render a `<ul>` element with proper indentation
fn render_ul(ul: &ElementRef, indent: usize, converter: &HtmlToMarkdown) -> String {
    let mut result = String::new();

    for child in ul.children() {
        if let Some(li_element) = ElementRef::wrap(child) {
            if li_element.value().name() == "li" {
                // Add indentation (4 spaces per level for markdown consistency)
                for _ in 0..indent {
                    result.push_str("    ");
                }

                // Add bullet marker
                result.push_str("* ");

                // Extract and convert the li content to markdown
                let li_html = li_element.html();
                let li_content = converter
                    .convert(&li_html)
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                // Remove the outer <li> tags that the converter might leave
                let li_content = li_content
                    .strip_prefix("*")
                    .unwrap_or(&li_content)
                    .trim();

                result.push_str(li_content);
                result.push('\n');
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::markdown;
    use scraper::{Html, Selector};

    fn test_converter() -> HtmlToMarkdown {
        markdown::build_converter("https://test.example.com")
    }

    #[test]
    fn test_simple_algorithm() {
        let html = r#"
            <ol>
                <li><p>First step</p></li>
                <li><p>Second step</p></li>
                <li><p>Third step</p></li>
            </ol>
        "#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());
        assert!(result.contains("1. First step"));
        assert!(result.contains("2. Second step"));
        assert!(result.contains("3. Third step"));
    }

    #[test]
    fn test_nested_algorithm() {
        let html = r#"
            <ol>
                <li><p>Step one</p></li>
                <li><p>Step two</p>
                    <ol>
                        <li><p>Sub-step 2.1</p></li>
                        <li><p>Sub-step 2.2</p></li>
                    </ol>
                </li>
                <li><p>Step three</p></li>
            </ol>
        "#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());
        assert!(result.contains("1. Step one"));
        assert!(result.contains("2. Step two"));
        assert!(result.contains("    1. Sub-step 2.1"));
        assert!(result.contains("    2. Sub-step 2.2"));
        assert!(result.contains("3. Step three"));
    }

    #[test]
    fn test_deeply_nested_algorithm() {
        let html = r#"
            <ol>
                <li><p>Level 1</p>
                    <ol>
                        <li><p>Level 1.1</p>
                            <ol>
                                <li><p>Level 1.1.1</p></li>
                            </ol>
                        </li>
                    </ol>
                </li>
            </ol>
        "#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());
        assert!(result.contains("1. Level 1"));
        assert!(result.contains("    1. Level 1.1"));
        assert!(result.contains("        1. Level 1.1.1"));
    }

    #[test]
    fn test_algorithm_with_var_and_code() {
        let html = r#"
            <ol>
                <li><p>Let <var>foo</var> be a <code>Document</code>.</p></li>
                <li><p>Return <var>foo</var>.</p></li>
            </ol>
        "#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());
        // <var> now renders as *italic* in markdown, <code> as `backtick`
        assert!(result.contains("1. Let *foo* be a `Document`."));
        assert!(result.contains("2. Return *foo*."));
    }

    #[test]
    fn test_algorithm_from_fixture() {
        let html = include_str!("../../tests/fixtures/algorithms/bikeshed_algorithm.html");
        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("div.algorithm ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());

        // Should have numbered steps
        assert!(result.contains("1. "));
        assert!(result.contains("2. "));

        // Check that it's not empty
        assert!(!result.trim().is_empty());
    }

    #[test]
    fn test_indentation() {
        let html = r#"
            <ol>
                <li><p>Top</p>
                    <ol>
                        <li><p>Nested</p></li>
                    </ol>
                </li>
            </ol>
        "#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());

        // Top level should have no indentation
        assert!(result.contains("1. Top"));

        // Nested numbered steps should have 4 spaces indentation per level
        assert!(result.contains("    1. Nested"));
        let lines: Vec<&str> = result.lines().collect();
        let nested_line = lines.iter().find(|l| l.contains("Nested")).unwrap();
        assert!(nested_line.starts_with("    1."));
    }

    #[test]
    fn test_note_between_steps() {
        // Notes/examples/warnings between steps should be formatted as blockquotes
        let html = r#"
            <ol>
                <li><p>First step</p></li>
                <li><p>Second step</p>
                    <div class="note">
                        <p>This is a note between steps.</p>
                    </div>
                </li>
                <li><p>Third step</p></li>
            </ol>
        "#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());

        // All three steps should be present
        assert!(result.contains("1. First step"));
        assert!(result.contains("2. Second step"));
        assert!(result.contains("3. Third step"));

        // Note should be formatted as blockquote with prefix and indented (continuation content)
        assert!(result.contains("    > **Note:** This is a note between steps."));

        // Third step should start on a new line after the blockquote
        let lines: Vec<&str> = result.lines().collect();
        let step3_index = lines.iter().position(|l| l.contains("3. Third step")).unwrap();
        let note_index = lines.iter().position(|l| l.contains("> **Note:**")).unwrap();

        // Step 3 should come after the note
        assert!(step3_index > note_index, "Step 3 should appear after the note");
    }

    #[test]
    fn test_nested_bullet_list() {
        // Test that nested <ul> lists are properly indented and in document order
        let html = r#"
            <ol>
                <li><p>If all of the following are true:</p>
                    <ul>
                        <li><var>x</var> is null;</li>
                        <li><var>y</var> is null;</li>
                    </ul>
                    <p>then return.</p>
                </li>
                <li><p>Next step</p></li>
            </ol>
        "#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("ol").unwrap();
        let ol = fragment.select(&selector).next().unwrap();

        let result = render_algorithm_ol(&ol, &test_converter());

        // Step 1 should contain the intro text
        assert!(result.contains("1. If all of the following are true:"));

        // Bullet items should be indented (4 spaces) and appear BEFORE "then return"
        assert!(result.contains("    * *x* is null;"));
        assert!(result.contains("    * *y* is null;"));

        // The "then return" should come AFTER the bullets
        let x_pos = result.find("*x* is null").expect("x bullet should exist");
        let y_pos = result.find("*y* is null").expect("y bullet should exist");
        let then_pos = result.find("then return").expect("then return should exist");

        assert!(x_pos < then_pos, "bullets should come before 'then return'");
        assert!(y_pos < then_pos, "bullets should come before 'then return'");

        // The "then return" should be indented (continuation content)
        assert!(result.contains("    then return"), "continuation content should be indented");

        // Step 2 should be present
        assert!(result.contains("2. Next step"));
    }
}

