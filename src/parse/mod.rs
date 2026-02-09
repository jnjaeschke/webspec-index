pub mod algorithms;
pub mod idl;
pub mod markdown;
pub mod references;
pub mod sections;

use crate::model::ParsedSpec;
use anyhow::Result;
use scraper::{Html, Selector};

/// Parse a complete spec HTML document into structured sections and references.
/// `base_url` is used to absolutize relative links in content markdown.
pub fn parse_spec(html: &str, spec_name: &str, base_url: &str) -> Result<ParsedSpec> {
    let document = Html::parse_document(html);
    let converter = markdown::build_converter(base_url);
    let mut sections = Vec::new();

    // Collect all potential section elements in a single pass to preserve document order
    // This includes: headings (h2-h6 with id) and definitions (dfn with id)
    let selector = Selector::parse("h2[id], h3[id], h4[id], h5[id], h6[id], dfn[id]")
        .map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;

    for element in document.select(&selector) {
        let tag_name = element.value().name();

        // Determine section type and create appropriate ParsedSection
        match tag_name {
            "h2" | "h3" | "h4" | "h5" | "h6" => {
                // This is a heading
                if let Some(section) = sections::parse_heading_element(&element, &converter)? {
                    sections.push(section);
                }
            }
            "dfn" => {
                // This is a definition, algorithm, or IDL - determine which
                if let Some(section) = sections::parse_dfn_element(&element, &converter)? {
                    sections.push(section);
                }
            }
            _ => {} // Ignore other elements
        }
    }

    // Build tree relationships (parent, prev, next)
    let sections = sections::build_section_tree(sections);

    // Extract references
    // Note: We need a SpecRegistry to resolve cross-spec URLs
    // For now, create an empty one (will be passed in later for full functionality)
    let registry = crate::spec_registry::SpecRegistry::new();
    let references = references::extract_references(html, spec_name, &sections, &registry);

    Ok(ParsedSpec {
        sections,
        references,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SectionType;

    #[test]
    fn test_parse_spec_full_pipeline() {
        let html = r#"
            <h2 id="intro">Introduction</h2>
            <p>This spec defines <dfn id="concept-widget">widgets</dfn>.</p>

            <h3 id="types">Widget Types</h3>
            <pre class="idl">
                <c- b>interface</c-> <dfn data-dfn-type="interface" id="widget"><code>Widget</code></dfn> {
                    <c- g>constructor</c->();
                };
            </pre>

            <div class="algorithm" data-algorithm="create widget">
                <p>To <dfn id="create-widget">create a widget</dfn>:</p>
                <ol>
                    <li>Let w be a new Widget.</li>
                    <li>Return w.</li>
                </ol>
            </div>

            <h3 id="examples">Examples</h3>
            <p>See the <dfn id="widget-example">widget example</dfn>.</p>
        "#;

        let parsed = parse_spec(html, "TEST", "https://test.example.com").unwrap();

        // Should have 7 sections total
        assert_eq!(parsed.sections.len(), 7);

        // Check section types and order
        assert_eq!(parsed.sections[0].anchor, "intro");
        assert_eq!(parsed.sections[0].section_type, SectionType::Heading);

        assert_eq!(parsed.sections[1].anchor, "concept-widget");
        assert_eq!(parsed.sections[1].section_type, SectionType::Definition);

        assert_eq!(parsed.sections[2].anchor, "types");
        assert_eq!(parsed.sections[2].section_type, SectionType::Heading);

        assert_eq!(parsed.sections[3].anchor, "widget");
        assert_eq!(parsed.sections[3].section_type, SectionType::Idl);

        assert_eq!(parsed.sections[4].anchor, "create-widget");
        assert_eq!(parsed.sections[4].section_type, SectionType::Algorithm);

        assert_eq!(parsed.sections[5].anchor, "examples");
        assert_eq!(parsed.sections[5].section_type, SectionType::Heading);

        assert_eq!(parsed.sections[6].anchor, "widget-example");
        assert_eq!(parsed.sections[6].section_type, SectionType::Definition);

        // Check tree relationships
        // intro (h2) should have no parent
        assert_eq!(parsed.sections[0].parent_anchor, None);

        // concept-widget (dfn) should have intro as parent
        assert_eq!(parsed.sections[1].parent_anchor, Some("intro".to_string()));

        // types (h3) should have intro as parent
        assert_eq!(parsed.sections[2].parent_anchor, Some("intro".to_string()));

        // widget (idl) should have types as parent
        assert_eq!(parsed.sections[3].parent_anchor, Some("types".to_string()));

        // create-widget (algorithm) should have types as parent
        assert_eq!(parsed.sections[4].parent_anchor, Some("types".to_string()));

        // examples (h3) should have intro as parent and types as prev sibling
        assert_eq!(parsed.sections[5].parent_anchor, Some("intro".to_string()));
        assert_eq!(parsed.sections[5].prev_anchor, Some("types".to_string()));

        // widget-example (dfn) should have examples as parent
        assert_eq!(parsed.sections[6].parent_anchor, Some("examples".to_string()));
    }

    #[test]
    fn test_parse_spec_empty() {
        let html = "<html><body></body></html>";
        let parsed = parse_spec(html, "TEST", "https://test.example.com").unwrap();
        assert_eq!(parsed.sections.len(), 0);
        assert_eq!(parsed.references.len(), 0);
    }
}
