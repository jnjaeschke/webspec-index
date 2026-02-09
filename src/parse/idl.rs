// IDL block extraction
use scraper::{ElementRef, Node};

/// Extract raw IDL text from a `<pre>` block, stripping syntax highlighting
/// Following DESIGN.md: strip `<c- ...>` tags but preserve whitespace exactly
pub fn extract_idl_text(pre_element: &ElementRef) -> String {
    let mut result = String::new();

    // Recursively collect text, stripping syntax highlighting tags
    fn collect_text_from_element(elem: &ElementRef, output: &mut String) {
        for child in elem.children() {
            if let Some(child_elem) = ElementRef::wrap(child) {
                // Recursively process child elements (but don't output the tags themselves)
                collect_text_from_element(&child_elem, output);
            } else if let Node::Text(text) = child.value() {
                output.push_str(text);
            }
        }
    }

    collect_text_from_element(pre_element, &mut result);

    // Trim trailing whitespace but preserve leading/internal whitespace
    result.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::{Html, Selector};

    #[test]
    fn test_extract_interface_idl() {
        let html = include_str!("../../tests/fixtures/idl/interface.html");
        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("pre").unwrap();
        let pre = fragment.select(&selector).next().unwrap();

        let result = extract_idl_text(&pre);

        // Should contain IDL keywords
        assert!(result.contains("interface"));
        assert!(result.contains("Event"));
        assert!(result.contains("constructor"));

        // Should NOT contain syntax highlighting tags
        assert!(!result.contains("<c-"));
        assert!(!result.contains("</c-"));
    }

    #[test]
    fn test_extract_dictionary_idl() {
        let html = include_str!("../../tests/fixtures/idl/dictionary.html");
        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("pre").unwrap();
        let pre = fragment.select(&selector).next().unwrap();

        let result = extract_idl_text(&pre);

        // Should contain dictionary IDL
        assert!(result.contains("dictionary"));
        assert!(result.contains("EventInit"));
        assert!(result.contains("boolean"));
        assert!(result.contains("bubbles"));
        assert!(result.contains("cancelable"));

        // Should NOT contain syntax highlighting
        assert!(!result.contains("<c-"));
    }

    #[test]
    fn test_preserves_whitespace() {
        let html = r#"<pre class="idl">interface Test {
  void method();
}</pre>"#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("pre").unwrap();
        let pre = fragment.select(&selector).next().unwrap();

        let result = extract_idl_text(&pre);

        // Should preserve the indentation
        assert!(result.contains("  void method();"));
    }

    #[test]
    fn test_strips_bikeshed_syntax_highlighting() {
        let html = r#"<pre class="idl"><c- b>interface</c-> <c- g>Test</c-> {
  <c- b>void</c-> <c- g>method</c->();
}</pre>"#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("pre").unwrap();
        let pre = fragment.select(&selector).next().unwrap();

        let result = extract_idl_text(&pre);

        // Should have clean IDL without tags
        assert_eq!(result.trim(), "interface Test {\n  void method();\n}");
    }

    #[test]
    fn test_strips_code_tags() {
        let html = r#"<pre class="idl">interface <code>Test</code> {
  void method();
}</pre>"#;

        let fragment = Html::parse_fragment(html);
        let selector = Selector::parse("pre").unwrap();
        let pre = fragment.select(&selector).next().unwrap();

        let result = extract_idl_text(&pre);

        // Should strip <code> tags
        assert_eq!(result.trim(), "interface Test {\n  void method();\n}");
    }
}
