// Cross-reference extraction from <a> elements
use crate::model::{ParsedReference, ParsedSection, SectionType};
use crate::spec_registry::SpecRegistry;
use scraper::Html;

/// Extract all cross-references from a parsed HTML document.
///
/// Uses a single document-order pass: walk all nodes, track the "current section"
/// (only headings and algorithms set it), and when we hit a link, attribute it to
/// that section.  Definition sub-sections are intentionally skipped for attribution
/// because they don't establish a new scope for algorithm steps and prose that
/// follow them.
pub fn extract_references(
    html: &str,
    spec_name: &str,
    sections: &[ParsedSection],
    registry: &SpecRegistry,
) -> Vec<ParsedReference> {
    let document = Html::parse_document(html);

    // Build lookup set of section anchors that establish reference scope.
    // Only headings and algorithms create persistent scope; definitions are
    // sub-sections that shouldn't override the enclosing algorithm/heading.
    let scope_anchors: std::collections::HashSet<&str> = sections
        .iter()
        .filter(|s| {
            matches!(
                s.section_type,
                SectionType::Heading | SectionType::Algorithm
            )
        })
        .map(|s| s.anchor.as_str())
        .collect();

    let mut seen = std::collections::HashSet::new();
    let mut references = Vec::new();
    let mut current_section: Option<String> = None;

    // Single document-order pass over all nodes
    for node_ref in document.root_element().descendants() {
        let Some(elem) = scraper::ElementRef::wrap(node_ref) else {
            continue;
        };

        // Check if this element defines a scope section
        if let Some(id) = elem.value().attr("id") {
            if scope_anchors.contains(id) {
                current_section = Some(id.to_string());
            }
        }

        // Check if this is a link worth recording
        if elem.value().name() == "a" {
            if let Some(href) = elem.value().attr("href") {
                if is_self_link(&elem) || is_biblio_ref(&elem) {
                    continue;
                }

                if let Some(ref section) = current_section {
                    if let Some(mut parsed_ref) = parse_href(href, section, registry) {
                        // Resolve intra-spec placeholder to the actual spec name
                        if parsed_ref.to_spec == "self" {
                            parsed_ref.to_spec = spec_name.to_string();
                        }

                        // Deduplicate by (from_anchor, to_spec, to_anchor)
                        let key = (
                            parsed_ref.from_anchor.clone(),
                            parsed_ref.to_spec.clone(),
                            parsed_ref.to_anchor.clone(),
                        );
                        if seen.insert(key) {
                            references.push(parsed_ref);
                        }
                    }
                }
            }
        }
    }

    references
}

/// Check if a link is a self-link (should be skipped)
fn is_self_link(link: &scraper::ElementRef) -> bool {
    let classes: Vec<_> = link.value().classes().collect();
    classes.contains(&"self-link")
}

/// Check if a link is a bibliography reference (should be skipped)
fn is_biblio_ref(link: &scraper::ElementRef) -> bool {
    if let Some(link_type) = link.value().attr("data-link-type") {
        link_type == "biblio"
    } else {
        false
    }
}

/// Parse an href attribute to determine the target spec and anchor
fn parse_href(href: &str, from_anchor: &str, registry: &SpecRegistry) -> Option<ParsedReference> {
    // Intra-spec reference (starts with #)
    if href.starts_with('#') {
        let to_anchor = href.trim_start_matches('#').to_string();
        return Some(ParsedReference {
            from_anchor: from_anchor.to_string(),
            to_spec: "self".to_string(),
            to_anchor,
        });
    }

    // Cross-spec reference (full URL)
    if href.starts_with("http://") || href.starts_with("https://") {
        // Try to resolve the URL using the registry
        if let Some((spec_name, anchor)) = registry.resolve_url(href) {
            return Some(ParsedReference {
                from_anchor: from_anchor.to_string(),
                to_spec: spec_name,
                to_anchor: anchor,
            });
        }
    }

    // Unknown or external URL, skip
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intra_spec_reference() {
        let html = r##"
            <h2 id="section1">Section 1</h2>
            <p>See <a href="#section2">Section 2</a> for details.</p>

            <h2 id="section2">Section 2</h2>
            <p>Content here.</p>
        "##;

        let sections = vec![
            ParsedSection {
                anchor: "section1".to_string(),
                title: Some("Section 1".to_string()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
            },
            ParsedSection {
                anchor: "section2".to_string(),
                title: Some("Section 2".to_string()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
            },
        ];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        // Should have one reference from section1 to section2
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].from_anchor, "section1");
        assert_eq!(refs[0].to_spec, "TEST");
        assert_eq!(refs[0].to_anchor, "section2");
    }

    #[test]
    fn test_skip_self_links() {
        let html = r##"
            <h2 id="section1">Section 1<a class="self-link" href="#section1"></a></h2>
            <p>Content here.</p>
        "##;

        let sections = vec![ParsedSection {
            anchor: "section1".to_string(),
            title: Some("Section 1".to_string()),
            content_text: None,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(2),
        }];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        // Should have no references (self-link skipped)
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_skip_biblio_refs() {
        let html = r##"
            <h2 id="section1">Section 1</h2>
            <p>See <a data-link-type="biblio" href="#biblio-infra">[INFRA]</a>.</p>
        "##;

        let sections = vec![ParsedSection {
            anchor: "section1".to_string(),
            title: Some("Section 1".to_string()),
            content_text: None,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(2),
        }];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        // Should have no references (biblio ref skipped)
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_cross_spec_reference() {
        let html = r##"
            <h2 id="section1">Section 1</h2>
            <p>See <a href="https://dom.spec.whatwg.org/#concept-tree">tree</a>.</p>
        "##;

        let sections = vec![ParsedSection {
            anchor: "section1".to_string(),
            title: Some("Section 1".to_string()),
            content_text: None,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(2),
        }];

        // SpecRegistry already includes WhatwgProvider
        let registry = SpecRegistry::new();

        let refs = extract_references(html, "TEST", &sections, &registry);

        // Should have one cross-spec reference
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].from_anchor, "section1");
        assert_eq!(refs[0].to_spec, "DOM");
        assert_eq!(refs[0].to_anchor, "concept-tree");
    }

    #[test]
    fn test_unknown_url_skipped() {
        let html = r##"
            <h2 id="section1">Section 1</h2>
            <p>See <a href="https://example.com/foo">external link</a>.</p>
        "##;

        let sections = vec![ParsedSection {
            anchor: "section1".to_string(),
            title: Some("Section 1".to_string()),
            content_text: None,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(2),
        }];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        // Should have no references (unknown URL skipped)
        assert_eq!(refs.len(), 0);
    }

    #[test]
    fn test_nested_sections() {
        let html = r##"
            <h2 id="parent">Parent</h2>
            <div>
                <h3 id="child">Child</h3>
                <p>See <a href="#parent">parent section</a>.</p>
            </div>
        "##;

        let sections = vec![
            ParsedSection {
                anchor: "parent".to_string(),
                title: Some("Parent".to_string()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: Some(2),
            },
            ParsedSection {
                anchor: "child".to_string(),
                title: Some("Child".to_string()),
                content_text: None,
                section_type: SectionType::Heading,
                parent_anchor: Some("parent".to_string()),
                prev_anchor: None,
                next_anchor: None,
                depth: Some(3),
            },
        ];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        // Should have one reference from child to parent
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].from_anchor, "child");
        assert_eq!(refs[0].to_anchor, "parent");
    }

    #[test]
    fn test_wattsi_algorithm_references() {
        // Test the Wattsi pattern where <dfn> and <a> are siblings in same <p>,
        // plus links in algorithm steps (sibling <ol>)
        let html = r##"
            <p>To <dfn id="navigate">navigate</dfn> a <a href="#navigable">navigable</a> to a
            <a href="https://url.spec.whatwg.org/#concept-url">URL</a>, with optional <a href="#post-resource">POST resource</a>:</p>
            <ol>
                <li><p>Let x be <a href="#snapshotting-params">snapshotting params</a>.</p></li>
                <li><p><a href="https://infra.spec.whatwg.org/#assert">Assert</a>: foo.</p></li>
            </ol>
        "##;

        let sections = vec![ParsedSection {
            anchor: "navigate".to_string(),
            title: Some("navigate".to_string()),
            content_text: None,
            section_type: SectionType::Algorithm,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: None,
        }];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        assert_eq!(refs.len(), 5, "Expected 5 references, got {}", refs.len());

        // All refs should be from "navigate"
        for ref_item in &refs {
            assert_eq!(ref_item.from_anchor, "navigate");
        }

        // Check intra-spec refs
        let intra: Vec<_> = refs
            .iter()
            .filter(|r| r.to_spec == "TEST")
            .map(|r| r.to_anchor.as_str())
            .collect();
        assert!(intra.contains(&"navigable"));
        assert!(intra.contains(&"post-resource"));
        assert!(intra.contains(&"snapshotting-params"));

        // Check cross-spec refs
        assert!(refs
            .iter()
            .any(|r| r.to_spec == "URL" && r.to_anchor == "concept-url"));
        assert!(refs
            .iter()
            .any(|r| r.to_spec == "INFRA" && r.to_anchor == "assert"));
    }

    #[test]
    fn test_algorithm_with_parameter_dfns() {
        // Real-world pattern: algorithm intro has parameter dfns that should NOT
        // steal attribution from the algorithm for links in the steps.
        let html = r##"
            <div data-algorithm="">
            <p>To <dfn id="navigate">navigate</dfn> a <a href="#navigable">navigable</a>
            using <dfn id="navigation-resource">documentResource</dfn> and
            <dfn id="navigation-response">response</dfn>:</p>
            <ol>
                <li><p><a href="#assert">Assert</a>: stuff.</p></li>
                <li><p>Let x be <a href="#snapshot">snapshot</a>.</p></li>
            </ol>
            </div>
        "##;

        let sections = vec![
            ParsedSection {
                anchor: "navigate".to_string(),
                title: Some("navigate".to_string()),
                content_text: None,
                section_type: SectionType::Algorithm,
                parent_anchor: None,
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
            ParsedSection {
                anchor: "navigation-resource".to_string(),
                title: Some("documentResource".to_string()),
                content_text: None,
                section_type: SectionType::Definition,
                parent_anchor: Some("navigate".to_string()),
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
            ParsedSection {
                anchor: "navigation-response".to_string(),
                title: Some("response".to_string()),
                content_text: None,
                section_type: SectionType::Definition,
                parent_anchor: Some("navigate".to_string()),
                prev_anchor: None,
                next_anchor: None,
                depth: None,
            },
        ];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        // ALL links (including those in <ol> steps) should be attributed to "navigate",
        // not to the parameter definitions
        assert_eq!(refs.len(), 3, "Expected 3 references, got {}", refs.len());
        for ref_item in &refs {
            assert_eq!(
                ref_item.from_anchor, "navigate",
                "Link to {} should be from navigate, not {}",
                ref_item.to_anchor, ref_item.from_anchor
            );
        }
    }

    #[test]
    fn test_cross_spec_reference_to_w3c() {
        let html = r##"
            <h2 id="section1">Section 1</h2>
            <p>See <a href="https://drafts.csswg.org/selectors-4/#specificity">specificity</a>.</p>
            <p>Also <a href="https://w3c.github.io/ServiceWorker/#service-worker-concept">SW</a>.</p>
        "##;

        let sections = vec![ParsedSection {
            anchor: "section1".to_string(),
            title: Some("Section 1".to_string()),
            content_text: None,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(2),
        }];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        assert_eq!(refs.len(), 2);
        assert!(refs
            .iter()
            .any(|r| r.to_spec == "CSS-SELECTORS" && r.to_anchor == "specificity"));
        assert!(refs
            .iter()
            .any(|r| r.to_spec == "SERVICE-WORKERS" && r.to_anchor == "service-worker-concept"));
    }

    #[test]
    fn test_duplicate_refs_deduplicated() {
        // Same anchor linked multiple times from the same section → single ref
        let html = r##"
            <h2 id="section1">Section 1</h2>
            <p>See <a href="#target">target</a> and also <a href="#target">target again</a>.</p>
        "##;

        let sections = vec![ParsedSection {
            anchor: "section1".to_string(),
            title: Some("Section 1".to_string()),
            content_text: None,
            section_type: SectionType::Heading,
            parent_anchor: None,
            prev_anchor: None,
            next_anchor: None,
            depth: Some(2),
        }];

        let registry = SpecRegistry::new();
        let refs = extract_references(html, "TEST", &sections, &registry);

        assert_eq!(refs.len(), 1, "Duplicate ref should be deduplicated");
        assert_eq!(refs[0].to_anchor, "target");
    }
}
