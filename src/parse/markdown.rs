// HTML-to-Markdown conversion using htmd with spec-aware custom handlers
use htmd::element_handler::Handlers;
use htmd::{Element, HtmlToMarkdown};

/// Build an htmd converter configured for spec content extraction.
/// `base_url` is used to absolutize relative `#anchor` links.
pub fn build_converter(base_url: &str) -> HtmlToMarkdown {
    let base = base_url.to_string();

    HtmlToMarkdown::builder()
        // Custom <a>: skip self-links/biblio, absolutize relative URLs
        .add_handler(
            vec!["a"],
            move |handlers: &dyn Handlers, element: Element| {
                let mut href: Option<String> = None;
                let mut is_self_link = false;
                let mut is_biblio = false;

                for attr in element.attrs.iter() {
                    let name = &attr.name.local;
                    if *name == *"href" {
                        href = Some(attr.value.to_string());
                    } else if *name == *"class" {
                        if has_class(&attr.value, "self-link") {
                            is_self_link = true;
                        }
                    } else if *name == *"data-link-type" && &*attr.value == "biblio" {
                        is_biblio = true;
                    }
                }

                if is_self_link {
                    return None;
                }

                let content = handlers.walk_children(element.node).content;

                if is_biblio {
                    return Some(content.into());
                }

                let Some(href) = href else {
                    return Some(content.into());
                };

                let url = if href.starts_with('#') {
                    format!("{}{}", base, href)
                } else {
                    href
                };

                Some(format!("[{}]({})", content, url).into())
            },
        )
        // <code> → `backtick` (handle links specially)
        .add_handler(vec!["code"], |handlers: &dyn Handlers, element: Element| {
            let content = handlers.walk_children(element.node).content;
            if content.is_empty() {
                return Some("".into());
            }
            // If content is a markdown link [text](url), extract and reformat as [`text`](url)
            if let Some((text, url)) = extract_markdown_link(&content) {
                Some(format!("[`{}`]({})", text, url).into())
            } else {
                Some(format!("`{}`", content).into())
            }
        })
        // <var> → *italic* (handle links specially)
        .add_handler(vec!["var"], |handlers: &dyn Handlers, element: Element| {
            let content = handlers.walk_children(element.node).content;
            if content.is_empty() {
                return Some("".into());
            }
            // If content is a markdown link [text](url), extract and reformat as [*text*](url)
            if let Some((text, url)) = extract_markdown_link(&content) {
                Some(format!("[*{}*]({})", text, url).into())
            } else {
                Some(format!("*{}*", content).into())
            }
        })
        // <dfn> → **bold**
        .add_handler(vec!["dfn"], |handlers: &dyn Handlers, element: Element| {
            let content = handlers.walk_children(element.node).content;
            if content.is_empty() {
                return Some("".into());
            }
            Some(format!("**{}**", content).into())
        })
        // <span>: strip secno, keep everything else
        .add_handler(vec!["span"], |handlers: &dyn Handlers, element: Element| {
            for attr in element.attrs.iter() {
                if *attr.name.local == *"class" && has_class(&attr.value, "secno") {
                    return None;
                }
            }
            Some(handlers.walk_children(element.node))
        })
        // <dl>: convert definition lists with class="props" to markdown tables
        .add_handler(vec!["dl"], |handlers: &dyn Handlers, element: Element| {
            let mut is_props = false;
            for attr in element.attrs.iter() {
                if *attr.name.local == *"class" && has_class(&attr.value, "props") {
                    is_props = true;
                    break;
                }
            }

            if is_props {
                // Parse it to extract dt/dd structure
                // Actually, let's build the table directly from walking the DOM
                Some(build_table_from_dl(element.node).into())
            } else {
                // Regular dl, just pass through
                Some(handlers.walk_children(element.node))
            }
        })
        // <div>, <dd>, <p>: detect note/example/warning/issue and format as blockquotes
        .add_handler(
            vec!["div", "dd", "p"],
            |handlers: &dyn Handlers, element: Element| {
                let mut prefix: Option<&str> = None;
                for attr in element.attrs.iter() {
                    if *attr.name.local == *"class" {
                        if has_class(&attr.value, "note") {
                            prefix = Some("**Note:** ");
                        } else if has_class(&attr.value, "example") {
                            prefix = Some("**Example:** ");
                        } else if has_class(&attr.value, "warning") {
                            prefix = Some("**Warning:** ");
                        } else if has_class(&attr.value, "XXX") || has_class(&attr.value, "issue") {
                            prefix = Some("**Issue:** ");
                        }
                        break;
                    }
                }

                let content = handlers.walk_children(element.node).content;

                if let Some(prefix) = prefix {
                    Some(to_blockquote(&content, prefix).into())
                } else {
                    // Regular element - for <p> tags, add newlines to mimic default behavior
                    if element.tag == "p" {
                        Some(format!("{}\n\n", content.trim()).into())
                    } else {
                        // For <div> and <dd>, just pass through content
                        Some(content.into())
                    }
                }
            },
        )
        .build()
}

/// Convert an HTML string to markdown with absolute URLs.
#[cfg(test)]
pub fn html_to_markdown(html: &str, base_url: &str) -> String {
    let converter = build_converter(base_url);
    converter.convert(html).unwrap_or_default()
}

/// Convert a scraper ElementRef's outer HTML to markdown.
pub fn element_to_markdown(element: &scraper::ElementRef, converter: &HtmlToMarkdown) -> String {
    let html = element.html();
    converter
        .convert(&html)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Convert raw HTML string to markdown.
pub fn element_to_markdown_from_html(html: &str, converter: &HtmlToMarkdown) -> String {
    converter
        .convert(html)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn has_class(attr_value: &str, class: &str) -> bool {
    attr_value.split_whitespace().any(|c| c == class)
}

/// Build a markdown table from a <dl> node by walking the DOM
fn build_table_from_dl(node: &std::rc::Rc<markup5ever_rcdom::Node>) -> String {
    use markup5ever_rcdom::NodeData;

    let mut rows = Vec::new();
    let mut current_dt: Option<String> = None;

    // Walk children to find dt/dd pairs
    for child in node.children.borrow().iter() {
        if let NodeData::Element { ref name, .. } = child.data {
            let tag_name = name.local.as_ref();

            match tag_name {
                "dt" => {
                    // Save previous dt if any
                    if let Some(term) = current_dt.take() {
                        rows.push((term, String::new()));
                    }
                    current_dt = Some(extract_text_recursive(child));
                }
                "dd" => {
                    let def = extract_text_recursive(child);
                    if let Some(term) = current_dt.take() {
                        rows.push((term, def));
                    }
                }
                _ => {}
            }
        }
    }

    // Handle leftover dt
    if let Some(term) = current_dt {
        rows.push((term, String::new()));
    }

    if rows.is_empty() {
        return String::new();
    }

    // Build markdown table
    let mut table = String::from("\n\n| Field | Value |\n|-------|-------|\n");
    for (term, def) in rows {
        let term = term.trim().replace('\n', " ");
        let def = def.trim().replace('\n', " ");
        table.push_str(&format!("| {} | {} |\n", term, def));
    }

    table
}

/// Recursively extract text from an rcdom node
fn extract_text_recursive(node: &std::rc::Rc<markup5ever_rcdom::Node>) -> String {
    use markup5ever_rcdom::NodeData;

    match &node.data {
        NodeData::Text { ref contents } => contents.borrow().to_string(),
        NodeData::Element { .. } | NodeData::Document => {
            let mut text = String::new();
            for child in node.children.borrow().iter() {
                text.push_str(&extract_text_recursive(child));
            }
            text
        }
        _ => String::new(),
    }
}

/// Extract text and URL from a markdown link: [text](url) → Some((text, url))
/// Returns None if the string is not a markdown link
fn extract_markdown_link(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    if !s.starts_with('[') {
        return None;
    }

    let close_bracket = s.find(']')?;
    let text = &s[1..close_bracket];

    let remaining = &s[close_bracket + 1..];
    if !remaining.starts_with('(') {
        return None;
    }

    let close_paren = remaining.find(')')?;
    let url = &remaining[1..close_paren];

    Some((text.to_string(), url.to_string()))
}

/// Convert text to markdown blockquote format with a prefix on the first line
fn to_blockquote(content: &str, prefix: &str) -> String {
    let content = content.trim();
    if content.is_empty() {
        return format!("\n\n> {}\n\n", prefix.trim());
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::new();

    // First line with prefix
    if let Some(first) = lines.first() {
        result.push_str(&format!("> {}{}\n", prefix, first));
    }

    // Remaining lines
    for line in &lines[1..] {
        if line.trim().is_empty() {
            result.push_str(">\n");
        } else {
            result.push_str(&format!("> {}\n", line));
        }
    }

    // Add spacing before and after the blockquote
    format!("\n\n{}\n\n", result.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_text() {
        let md = html_to_markdown("<p>Hello world.</p>", "https://example.com");
        assert_eq!(md, "Hello world.");
    }

    #[test]
    fn test_var_italic() {
        let md = html_to_markdown("<p>Let <var>x</var> be 1.</p>", "https://example.com");
        assert_eq!(md, "Let *x* be 1.");
    }

    #[test]
    fn test_dfn_bold() {
        let md = html_to_markdown(
            "<p>A <dfn id=\"tree\">tree</dfn> is a structure.</p>",
            "https://example.com",
        );
        assert_eq!(md, "A **tree** is a structure.");
    }

    #[test]
    fn test_code_backtick() {
        let md = html_to_markdown(
            "<p>The <code>Document</code> interface.</p>",
            "https://example.com",
        );
        assert_eq!(md, "The `Document` interface.");
    }

    #[test]
    fn test_relative_link_absolutized() {
        let md = html_to_markdown(
            r##"<p>See <a href="#foo">foo</a>.</p>"##,
            "https://html.spec.whatwg.org",
        );
        assert_eq!(md, "See [foo](https://html.spec.whatwg.org#foo).");
    }

    #[test]
    fn test_absolute_link_preserved() {
        let md = html_to_markdown(
            r##"<p>See <a href="https://dom.spec.whatwg.org/#concept-tree">tree</a>.</p>"##,
            "https://html.spec.whatwg.org",
        );
        assert_eq!(md, "See [tree](https://dom.spec.whatwg.org/#concept-tree).");
    }

    #[test]
    fn test_code_wrapping_link() {
        // <code><a href="...">text</a></code> should become [`text`](url)
        // NOT `[text](url)` which breaks the link
        let md = html_to_markdown(
            r##"<p>The <code><a href="#document">Document</a></code> interface.</p>"##,
            "https://html.spec.whatwg.org",
        );
        assert_eq!(
            md,
            "The [`Document`](https://html.spec.whatwg.org#document) interface."
        );
    }

    #[test]
    fn test_var_wrapping_link() {
        // <var><a href="...">text</a></var> should become [*text*](url)
        let md = html_to_markdown(
            r##"<p>Let <var><a href="#x">x</a></var> be a variable.</p>"##,
            "https://html.spec.whatwg.org",
        );
        assert_eq!(
            md,
            "Let [*x*](https://html.spec.whatwg.org#x) be a variable."
        );
    }

    #[test]
    fn test_self_link_stripped() {
        let md = html_to_markdown(
            r##"<h2 id="foo">Heading<a class="self-link" href="#foo"></a></h2>"##,
            "https://example.com",
        );
        assert_eq!(md, "## Heading");
    }

    #[test]
    fn test_biblio_ref_text_only() {
        let md = html_to_markdown(
            r##"<p>See <a data-link-type="biblio" href="#biblio-infra">[INFRA]</a>.</p>"##,
            "https://example.com",
        );
        assert_eq!(md, r"See \[INFRA\].");
    }

    #[test]
    fn test_secno_stripped() {
        let md = html_to_markdown(
            r##"<h2><span class="secno">1.2 </span>Introduction</h2>"##,
            "https://example.com",
        );
        assert_eq!(md, "## Introduction");
    }

    #[test]
    fn test_bikeshed_syntax_highlighting() {
        let md = html_to_markdown(
            "<p><c- b>interface</c-> <c- g>Event</c-></p>",
            "https://example.com",
        );
        assert_eq!(md, "interface Event");
    }

    #[test]
    fn test_mixed_inline() {
        let md = html_to_markdown(
            r##"<p>To <dfn id="navigate">navigate</dfn> a <a href="#navigable">navigable</a> using <var>url</var>:</p>"##,
            "https://html.spec.whatwg.org",
        );
        assert_eq!(
            md,
            "To **navigate** a [navigable](https://html.spec.whatwg.org#navigable) using *url*:"
        );
    }

    #[test]
    fn test_note_block() {
        let md = html_to_markdown(
            r##"<p>Some text.</p><div class="note"><p>This is a note.</p></div><p>More text.</p>"##,
            "https://example.com",
        );
        assert_eq!(
            md,
            "Some text.\n\n> **Note:** This is a note.\n\nMore text."
        );
    }

    #[test]
    fn test_example_block() {
        let md = html_to_markdown(
            r##"<div class="example"><p>This is an example.</p></div>"##,
            "https://example.com",
        );
        assert!(md.contains("> **Example:** This is an example."));
    }

    #[test]
    fn test_warning_block() {
        let md = html_to_markdown(
            r##"<div class="warning"><p>This is a warning.</p></div>"##,
            "https://example.com",
        );
        assert!(md.contains("> **Warning:** This is a warning."));
    }

    #[test]
    fn test_note_block_multiline() {
        let md = html_to_markdown(
            r##"<div class="note"><p>First paragraph.</p><p>Second paragraph.</p></div>"##,
            "https://example.com",
        );
        // Multi-paragraph note should have > on each line
        assert!(md.contains("> **Note:** First paragraph."));
        assert!(md.contains(">\n> Second paragraph."));
    }

    #[test]
    fn test_dl_props_to_table() {
        let md = html_to_markdown(
            r##"<dl class="props"><dt>field1</dt><dd>value1</dd><dt>field2</dt><dd>value2</dd></dl>"##,
            "https://example.com",
        );
        assert!(md.contains("| Field | Value |"));
        assert!(md.contains("|-------|-------|"));
        assert!(md.contains("| field1 | value1 |"));
        assert!(md.contains("| field2 | value2 |"));
    }

    #[test]
    fn test_dl_props_with_links() {
        let md = html_to_markdown(
            r##"<dl class="props"><dt><a href="#foo">term</a></dt><dd>definition with <var>variable</var></dd></dl>"##,
            "https://html.spec.whatwg.org",
        );
        assert!(md.contains("| Field | Value |"));
        assert!(md.contains("| term | definition with variable |"));
    }

    #[test]
    fn test_regular_dl_not_converted() {
        // Regular dl without class="props" should not be converted to table
        let md = html_to_markdown(
            r##"<dl><dt>term</dt><dd>definition</dd></dl>"##,
            "https://example.com",
        );
        // Should NOT contain table markers
        assert!(!md.contains("| Field | Value |"));
    }

    #[test]
    fn test_full_algorithm_markdown() {
        // Integration test: full parse pipeline produces markdown content
        let html = r##"
            <div data-algorithm="">
            <p>To <dfn id="navigate">navigate</dfn> a <a href="#navigable">navigable</a>
            to a <a href="https://url.spec.whatwg.org/#concept-url">URL</a> <var>url</var>:</p>
            <ol>
                <li><p>Let <var>x</var> be <a href="#snapshotting-params">snapshotting params</a>.</p></li>
                <li><p><a href="https://infra.spec.whatwg.org/#assert">Assert</a>: <var>x</var> is not null.</p></li>
            </ol>
            </div>
        "##;

        let parsed =
            crate::parse::parse_spec(html, "TEST", "https://html.spec.whatwg.org").unwrap();

        let algo = parsed
            .sections
            .iter()
            .find(|s| s.anchor == "navigate")
            .expect("navigate section should exist");

        let content = algo.content_text.as_ref().expect("should have content");

        // Intro should have bold dfn, absolute links, and italic var
        assert!(content.contains("**navigate**"), "dfn should be bold");
        assert!(
            content.contains("[navigable](https://html.spec.whatwg.org#navigable)"),
            "relative link should be absolutized"
        );
        assert!(
            content.contains("[URL](https://url.spec.whatwg.org/#concept-url)"),
            "absolute link should be preserved"
        );
        assert!(content.contains("*url*"), "var should be italic");

        // Steps should have hierarchical numbering with markdown
        assert!(content.contains("1. "), "should have step 1");
        assert!(content.contains("2. "), "should have step 2");
        assert!(
            content.contains(
                "[snapshotting params](https://html.spec.whatwg.org#snapshotting-params)"
            ),
            "step link should be absolutized"
        );
        assert!(
            content.contains("[Assert](https://infra.spec.whatwg.org/#assert)"),
            "cross-spec link should be preserved"
        );
    }
}
