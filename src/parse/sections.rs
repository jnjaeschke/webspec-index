use crate::model::{ParsedSection, SectionType};
use anyhow::Result;
use htmd::HtmlToMarkdown;
#[cfg(test)]
use scraper::{Html, Selector};

/// Extract content between a heading and the next section (heading or dfn)
/// Returns the markdown-converted prose content
fn extract_heading_content(
    heading: &scraper::ElementRef,
    current_depth: u8,
    converter: &HtmlToMarkdown,
) -> Option<String> {
    use super::markdown;

    let mut content_html = String::new();
    let mut current = heading.next_sibling();

    while let Some(node) = current {
        if let Some(sibling_elem) = scraper::ElementRef::wrap(node) {
            let tag_name = sibling_elem.value().name();

            // Stop at next heading of same or higher level
            if let Some(sibling_depth) = heading_depth(tag_name) {
                if sibling_depth <= current_depth {
                    break;
                }
            }

            // Stop at definitions (they're separate sections)
            if tag_name == "dfn" && sibling_elem.value().attr("id").is_some() {
                break;
            }

            // Collect this element's HTML
            content_html.push_str(&sibling_elem.html());
        }

        current = node.next_sibling();
    }

    if content_html.trim().is_empty() {
        return None;
    }

    let markdown = markdown::element_to_markdown_from_html(&content_html, converter);
    let trimmed = markdown.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Extract title text from a heading element, stripping secno and self-link
fn extract_heading_title(element: &scraper::ElementRef) -> Option<String> {
    // Clone the element to manipulate it
    let mut text_parts = Vec::new();

    for node in element.children() {
        if let Some(elem) = scraper::ElementRef::wrap(node) {
            // Skip <span class="secno"> and <a class="self-link">
            let classes = elem.value().classes().collect::<Vec<_>>();
            if classes.contains(&"secno") || classes.contains(&"self-link") {
                continue;
            }
            // Get text from other elements (like <span class="content">)
            text_parts.push(elem.text().collect::<String>());
        } else if let Some(text) = node.value().as_text() {
            text_parts.push(text.to_string());
        }
    }

    let result = text_parts.join("").trim().to_string();
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Get the depth (2-6) from a heading tag name
fn heading_depth(tag: &str) -> Option<u8> {
    match tag {
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Parse a single heading element into a ParsedSection
pub fn parse_heading_element(
    element: &scraper::ElementRef,
    converter: &HtmlToMarkdown,
) -> Result<Option<ParsedSection>> {
    let anchor = match element.value().attr("id") {
        Some(id) => id.to_string(),
        None => return Ok(None), // No id, skip this heading
    };

    let title = extract_heading_title(element);
    let depth = heading_depth(element.value().name())
        .ok_or_else(|| anyhow::anyhow!("Invalid heading tag: {}", element.value().name()))?;

    // Extract content between this heading and the next heading/definition
    let content_text = extract_heading_content(element, depth, converter);

    Ok(Some(ParsedSection {
        anchor,
        title,
        content_text,
        section_type: SectionType::Heading,
        parent_anchor: None,
        prev_anchor: None,
        next_anchor: None,
        depth: Some(depth),
    }))
}

/// Parse a single dfn element into a ParsedSection
/// Determines whether it's a Definition, Algorithm, or IDL based on context
pub fn parse_dfn_element(
    element: &scraper::ElementRef,
    converter: &HtmlToMarkdown,
) -> Result<Option<ParsedSection>> {
    let anchor = match element.value().attr("id") {
        Some(id) => id.to_string(),
        None => return Ok(None), // No id, skip this dfn
    };

    // Skip dfns that are inside algorithm content (e.g., inside <ol> steps)
    // These are part of the algorithm's markdown content, not separate sections
    if is_inside_algorithm_content(element) {
        return Ok(None);
    }

    // Skip parameter dfns:
    // 1. Those with data-dfn-for but WITHOUT data-dfn-type (e.g., <dfn data-dfn-for="navigate">url</dfn>)
    // 2. Those with <var> as direct child (e.g., <dfn><var>options</var></dfn>)
    // BUT keep method/attribute dfns which have BOTH data-dfn-for AND data-dfn-type
    // Example of PARAMETER (skip): <dfn data-dfn-for="navigate"><var>url</var></dfn>
    // Example of PARAMETER (skip): <dfn><var>options</var></dfn>
    // Example of METHOD (keep): <dfn data-dfn-for="HTMLSlotElement" data-dfn-type="method">assign(...)</dfn>
    let has_dfn_for = element.value().attr("data-dfn-for").is_some();
    let has_dfn_type = element.value().attr("data-dfn-type").is_some();
    let has_direct_var_child = element
        .children()
        .filter_map(scraper::ElementRef::wrap)
        .any(|c| c.value().name() == "var");

    // Skip if it's a parameter dfn
    if (has_dfn_for && !has_dfn_type) || has_direct_var_child {
        return Ok(None);
    }

    // Skip argument dfns (data-dfn-type="argument" in Bikeshed-generated specs)
    // These are WebIDL function parameters, not standalone queryable concepts
    if element.value().attr("data-dfn-type") == Some("argument") {
        return Ok(None);
    }

    // Extract text content (including nested elements like <code>)
    let title = element.text().collect::<String>().trim().to_string();
    let title = if title.is_empty() { None } else { Some(title) };

    // Determine section type based on context
    // (parameter dfns already skipped above)
    let section_type = if is_inside_algorithm_div(element) {
        SectionType::Algorithm
    } else if is_idl_type(element) {
        SectionType::Idl
    } else {
        SectionType::Definition
    };

    // Extract content based on section type
    let content_text = match section_type {
        SectionType::Definition => extract_definition_content(element, converter),
        SectionType::Algorithm => extract_algorithm_content(element, converter),
        SectionType::Idl => extract_idl_content(element),
        _ => None,
    };

    Ok(Some(ParsedSection {
        anchor,
        title,
        content_text,
        section_type,
        parent_anchor: None,
        prev_anchor: None,
        next_anchor: None,
        depth: None,
    }))
}

/// Extract content for a definition (dfn not in algorithm, not IDL)
/// Finds the enclosing block-level element and converts to markdown
fn extract_definition_content(
    element: &scraper::ElementRef,
    converter: &HtmlToMarkdown,
) -> Option<String> {
    use super::markdown;

    // Find the enclosing block-level element (p, div, dd, etc.)
    let mut current = element.parent();
    while let Some(node) = current {
        if let Some(parent_elem) = scraper::ElementRef::wrap(node) {
            let tag_name = parent_elem.value().name();
            // Block-level elements that can contain definitions
            if matches!(tag_name, "p" | "div" | "dd" | "dt" | "li" | "section") {
                return Some(markdown::element_to_markdown(&parent_elem, converter));
            }
        }
        current = node.parent();
    }

    // Fallback: just use the dfn's text
    Some(element.text().collect::<String>().trim().to_string())
}

/// Extract content for an algorithm (dfn inside div.algorithm or with sibling <ol>)
/// Handles both Bikeshed (div.algorithm) and Wattsi (sibling ol) patterns
fn extract_algorithm_content(
    element: &scraper::ElementRef,
    converter: &HtmlToMarkdown,
) -> Option<String> {
    use super::{algorithms, markdown};

    let mut current = element.parent();
    while let Some(node) = current {
        if let Some(parent_elem) = scraper::ElementRef::wrap(node) {
            // Bikeshed/Wattsi div pattern: div.algorithm or div[data-algorithm]
            if parent_elem.value().name() == "div" {
                let classes: Vec<_> = parent_elem.value().classes().collect();
                let is_algo_div = classes.contains(&"algorithm")
                    || parent_elem.value().attr("data-algorithm").is_some();
                if is_algo_div {
                    return extract_from_algorithm_div(&parent_elem, converter);
                }
            }

            // Wattsi sibling pattern: <p>To <dfn>foo</dfn>:</p><ol>...</ol>
            if matches!(parent_elem.value().name(), "p" | "dd" | "li") {
                let intro = markdown::element_to_markdown(&parent_elem, converter);

                let mut sibling = node.next_sibling();
                while let Some(sib_node) = sibling {
                    if let Some(sib_elem) = scraper::ElementRef::wrap(sib_node) {
                        if sib_elem.value().name() == "ol" {
                            let steps = algorithms::render_algorithm_ol(&sib_elem, converter);
                            return Some(format!("{}\n\n{}", intro.trim(), steps));
                        }
                        if matches!(
                            sib_elem.value().name(),
                            "p" | "div" | "h2" | "h3" | "h4" | "h5" | "h6"
                        ) {
                            break;
                        }
                    }
                    sibling = sib_node.next_sibling();
                }
            }
        }
        current = node.parent();
    }

    None
}

/// Extract algorithm content from a div.algorithm or div[data-algorithm] container.
/// Properly separates the intro paragraph(s) from the steps <ol>.
fn extract_from_algorithm_div(
    div: &scraper::ElementRef,
    converter: &HtmlToMarkdown,
) -> Option<String> {
    use super::algorithms;

    let ol_selector = scraper::Selector::parse("ol").ok()?;
    let ol_elem = div.select(&ol_selector).next()?;

    // Build intro HTML from children before the first <ol>
    let mut intro_html = String::new();
    for child in div.children() {
        if let Some(child_elem) = scraper::ElementRef::wrap(child) {
            if child_elem.value().name() == "ol" {
                break;
            }
            intro_html.push_str(&child_elem.html());
        } else if let Some(text) = child.value().as_text() {
            intro_html.push_str(text);
        }
    }

    let intro = converter
        .convert(&intro_html)
        .unwrap_or_default()
        .trim()
        .to_string();
    let steps = algorithms::render_algorithm_ol(&ol_elem, converter);
    Some(format!("{}\n\n{}", intro, steps))
}

/// Extract content for an IDL type (dfn with data-dfn-type)
/// Finds the parent <pre> block and extracts IDL
fn extract_idl_content(element: &scraper::ElementRef) -> Option<String> {
    use super::idl;

    // Find the parent <pre> element
    let mut current = element.parent();
    while let Some(node) = current {
        if let Some(parent_elem) = scraper::ElementRef::wrap(node) {
            if parent_elem.value().name() == "pre" {
                let idl_text = idl::extract_idl_text(&parent_elem);
                return Some(idl_text);
            }
        }
        current = node.parent();
    }

    None
}

/// Collect all ID'd headings from HTML
#[cfg(test)]
pub fn collect_headings(html: &str) -> Result<Vec<ParsedSection>> {
    let document = Html::parse_document(html);
    let converter = crate::parse::markdown::build_converter("https://test.example.com");
    let mut sections = Vec::new();

    // Select all headings with an id attribute (h2, h3, h4, h5, h6)
    let selector = Selector::parse("h2[id], h3[id], h4[id], h5[id], h6[id]")
        .map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;

    for element in document.select(&selector) {
        if let Some(section) = parse_heading_element(&element, &converter)? {
            // Clear content for tests that expect None (tree building tests)
            // Real parsing in parse_spec will extract content
            sections.push(ParsedSection {
                content_text: None,
                ..section
            });
        }
    }

    Ok(sections)
}

/// Check if a dfn is inside an algorithm's <ol> content (i.e., part of the algorithm steps)
/// These dfns should not be collected as separate sections - they're part of algorithm content
fn is_inside_algorithm_content(element: &scraper::ElementRef) -> bool {
    // Check if this element is inside an <ol>
    let mut current = element.parent();
    while let Some(node) = current {
        if let Some(parent_elem) = scraper::ElementRef::wrap(node) {
            if parent_elem.value().name() == "ol" {
                // Found an <ol> ancestor. Now check if this <ol> is part of an algorithm.
                // Two patterns:
                // 1. Bikeshed: <div class="algorithm">...<ol>...</ol></div>
                // 2. Wattsi: <p>To <dfn>foo</dfn>:</p><ol>...</ol> (sibling pattern)

                // Check if <ol> is inside div.algorithm or div[data-algorithm]
                let mut ol_ancestor = parent_elem.parent();
                while let Some(anc_node) = ol_ancestor {
                    if let Some(anc_elem) = scraper::ElementRef::wrap(anc_node) {
                        if anc_elem.value().name() == "div" {
                            let classes: Vec<_> = anc_elem.value().classes().collect();
                            if classes.contains(&"algorithm")
                                || anc_elem.value().attr("data-algorithm").is_some()
                            {
                                return true; // Inside Bikeshed/Wattsi div.algorithm pattern
                            }
                        }
                    }
                    ol_ancestor = anc_node.parent();
                }

                // Check Wattsi sibling pattern: preceding <p> contains algorithm-defining dfn
                let mut prev_sibling = node.prev_sibling();
                while let Some(prev_node) = prev_sibling {
                    if let Some(prev_elem) = scraper::ElementRef::wrap(prev_node) {
                        if matches!(prev_elem.value().name(), "p" | "dd" | "li") {
                            // Check if this block contains a dfn (algorithm-defining)
                            if let Ok(dfn_selector) = scraper::Selector::parse("dfn[id]") {
                                if prev_elem.select(&dfn_selector).next().is_some() {
                                    return true; // Wattsi sibling pattern detected
                                }
                            }
                        }
                        // Stop at block elements
                        if matches!(
                            prev_elem.value().name(),
                            "p" | "div" | "h2" | "h3" | "h4" | "h5" | "h6"
                        ) {
                            break;
                        }
                    }
                    prev_sibling = prev_node.prev_sibling();
                }

                // <ol> is not part of an algorithm, so this dfn is not in algorithm content
                return false;
            }
        }
        current = node.parent();
    }
    false
}

/// Check if an element is inside a <div class="algorithm"> or followed by sibling <ol>
/// Detects both Bikeshed style (div.algorithm wrapping) and Wattsi style (sibling ol)
fn is_inside_algorithm_div(element: &scraper::ElementRef) -> bool {
    // First check Bikeshed pattern: parent div.algorithm
    let mut current = element.parent();
    while let Some(node) = current {
        if let Some(parent_elem) = scraper::ElementRef::wrap(node) {
            if parent_elem.value().name() == "div" {
                let classes: Vec<_> = parent_elem.value().classes().collect();
                if classes.contains(&"algorithm") {
                    return true;
                }
            }

            // Also check Wattsi pattern: if this block element has a sibling <ol>
            // (e.g., <p>To <dfn>foo</dfn>:</p><ol>...</ol>)
            if matches!(parent_elem.value().name(), "p" | "div" | "dd" | "li") {
                // Check if there's a following <ol> sibling
                let mut sibling = node.next_sibling();
                while let Some(sib_node) = sibling {
                    if let Some(sib_elem) = scraper::ElementRef::wrap(sib_node) {
                        if sib_elem.value().name() == "ol" {
                            return true;
                        }
                        // Stop if we hit another block element (not whitespace)
                        if matches!(
                            sib_elem.value().name(),
                            "p" | "div" | "h2" | "h3" | "h4" | "h5" | "h6"
                        ) {
                            break;
                        }
                    }
                    sibling = sib_node.next_sibling();
                }
            }
        }
        current = node.parent();
    }
    false
}

/// Check if a dfn element is an IDL type definition
fn is_idl_type(element: &scraper::ElementRef) -> bool {
    if let Some(dfn_type) = element.value().attr("data-dfn-type") {
        matches!(
            dfn_type,
            "interface" | "dictionary" | "enum" | "callback" | "callback interface" | "typedef"
        )
    } else {
        false
    }
}

/// Collect all ID'd IDL type definitions from HTML
#[cfg(test)]
pub fn collect_idl(html: &str) -> Result<Vec<ParsedSection>> {
    let document = Html::parse_document(html);
    let mut sections = Vec::new();

    // Select all dfn elements with an id and data-dfn-type attribute
    let selector = Selector::parse("dfn[id][data-dfn-type]")
        .map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;

    for element in document.select(&selector) {
        // Only collect IDL type definitions (interface, dictionary, enum, etc.)
        if !is_idl_type(&element) {
            continue;
        }

        let anchor = element
            .value()
            .attr("id")
            .ok_or_else(|| anyhow::anyhow!("IDL type missing id"))?
            .to_string();

        // Extract text content (including nested elements like <code>)
        let title = element.text().collect::<String>().trim().to_string();
        let title = if title.is_empty() { None } else { Some(title) };

        sections.push(ParsedSection {
            anchor,
            title,
            content_text: None, // Will be extracted in a later pass
            section_type: SectionType::Idl,
            parent_anchor: None, // Will be computed in tree building
            prev_anchor: None,   // Will be computed in tree building
            next_anchor: None,   // Will be computed in tree building
            depth: None,         // IDL types don't have depth
        });
    }

    Ok(sections)
}

/// Collect all ID'd algorithms from HTML (dfn elements inside div.algorithm)
#[cfg(test)]
pub fn collect_algorithms(html: &str) -> Result<Vec<ParsedSection>> {
    let document = Html::parse_document(html);
    let mut sections = Vec::new();

    // Select all definitions with an id attribute inside algorithm divs
    let selector = Selector::parse("div.algorithm dfn[id]")
        .map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;

    for element in document.select(&selector) {
        let anchor = element
            .value()
            .attr("id")
            .ok_or_else(|| anyhow::anyhow!("Algorithm missing id"))?
            .to_string();

        // Extract text content (including nested elements like <code>)
        let title = element.text().collect::<String>().trim().to_string();
        let title = if title.is_empty() { None } else { Some(title) };

        sections.push(ParsedSection {
            anchor,
            title,
            content_text: None, // Will be extracted in a later pass
            section_type: SectionType::Algorithm,
            parent_anchor: None, // Will be computed in tree building
            prev_anchor: None,   // Will be computed in tree building
            next_anchor: None,   // Will be computed in tree building
            depth: None,         // Algorithms don't have depth
        });
    }

    Ok(sections)
}

/// Collect all ID'd definitions from HTML (dfn elements NOT inside div.algorithm and NOT IDL types)
#[cfg(test)]
pub fn collect_definitions(html: &str) -> Result<Vec<ParsedSection>> {
    let document = Html::parse_document(html);
    let mut sections = Vec::new();

    // Select all definitions with an id attribute
    let selector =
        Selector::parse("dfn[id]").map_err(|e| anyhow::anyhow!("Invalid selector: {:?}", e))?;

    for element in document.select(&selector) {
        // Skip definitions that are inside algorithm divs (those are algorithms)
        if is_inside_algorithm_div(&element) {
            continue;
        }

        // Skip IDL type definitions (those are IDL)
        if is_idl_type(&element) {
            continue;
        }

        let anchor = element
            .value()
            .attr("id")
            .ok_or_else(|| anyhow::anyhow!("Definition missing id"))?
            .to_string();

        // Extract text content (including nested elements like <code>)
        let title = element.text().collect::<String>().trim().to_string();
        let title = if title.is_empty() { None } else { Some(title) };

        sections.push(ParsedSection {
            anchor,
            title,
            content_text: None, // Will be extracted in a later pass
            section_type: SectionType::Definition,
            parent_anchor: None, // Will be computed in tree building
            prev_anchor: None,   // Will be computed in tree building
            next_anchor: None,   // Will be computed in tree building
            depth: None,         // Definitions don't have depth
        });
    }

    Ok(sections)
}

/// Build parent/child/sibling relationships for a flat list of sections
pub fn build_section_tree(mut sections: Vec<ParsedSection>) -> Vec<ParsedSection> {
    // First pass: compute parent relationships
    for i in 0..sections.len() {
        if let Some(current_depth) = sections[i].depth {
            // This is a heading - find parent heading with depth < current
            for j in (0..i).rev() {
                if let Some(parent_depth) = sections[j].depth {
                    if parent_depth < current_depth {
                        sections[i].parent_anchor = Some(sections[j].anchor.clone());
                        break;
                    }
                }
            }
        } else {
            // This is a non-heading (definition, algorithm, IDL)
            // Parent is the most recent heading (any heading)
            for j in (0..i).rev() {
                if sections[j].depth.is_some() {
                    sections[i].parent_anchor = Some(sections[j].anchor.clone());
                    break;
                }
            }
        }
    }

    // Second pass: compute prev/next sibling relationships
    for i in 0..sections.len() {
        let current_depth = sections[i].depth;
        let current_parent = sections[i].parent_anchor.clone();

        // Look backwards for prev sibling (same depth, same parent)
        for j in (0..i).rev() {
            if sections[j].depth == current_depth && sections[j].parent_anchor == current_parent {
                sections[i].prev_anchor = Some(sections[j].anchor.clone());
                break;
            }
        }

        // Look forwards for next sibling (same depth, same parent)
        for j in (i + 1)..sections.len() {
            if sections[j].depth == current_depth && sections[j].parent_anchor == current_parent {
                sections[i].next_anchor = Some(sections[j].anchor.clone());
                break;
            }
        }
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bikeshed_heading_parsing() {
        let html = include_str!("../../tests/fixtures/headings/bikeshed_heading.html");
        let sections = collect_headings(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "trees");
        assert_eq!(section.title, Some("Trees".to_string()));
        assert_eq!(section.section_type, SectionType::Heading);
        assert_eq!(section.depth, Some(3));
    }

    #[test]
    fn test_wattsi_heading_parsing() {
        let html = include_str!("../../tests/fixtures/headings/wattsi_heading.html");
        let sections = collect_headings(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "abstract");
        assert_eq!(
            section.title,
            Some("Where does this specification fit?".to_string())
        );
        assert_eq!(section.section_type, SectionType::Heading);
        assert_eq!(section.depth, Some(3));
    }

    #[test]
    fn test_multiple_heading_levels() {
        let html = r#"
            <h2 id="section-1">Section 1</h2>
            <h3 id="section-1-1">Section 1.1</h3>
            <h4 id="section-1-1-1">Section 1.1.1</h4>
            <h2 id="section-2">Section 2</h2>
        "#;

        let sections = collect_headings(html).unwrap();
        assert_eq!(sections.len(), 4);

        assert_eq!(sections[0].anchor, "section-1");
        assert_eq!(sections[0].depth, Some(2));

        assert_eq!(sections[1].anchor, "section-1-1");
        assert_eq!(sections[1].depth, Some(3));

        assert_eq!(sections[2].anchor, "section-1-1-1");
        assert_eq!(sections[2].depth, Some(4));

        assert_eq!(sections[3].anchor, "section-2");
        assert_eq!(sections[3].depth, Some(2));
    }

    #[test]
    fn test_heading_without_id_ignored() {
        let html = r#"
            <h2 id="has-id">With ID</h2>
            <h2>Without ID</h2>
        "#;

        let sections = collect_headings(html).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].anchor, "has-id");
    }

    #[test]
    fn test_build_section_tree_simple_nesting() {
        let html = r#"
            <h2 id="s1">Section 1</h2>
            <h3 id="s1-1">Section 1.1</h3>
            <h3 id="s1-2">Section 1.2</h3>
            <h4 id="s1-2-1">Section 1.2.1</h4>
            <h2 id="s2">Section 2</h2>
        "#;

        let sections = collect_headings(html).unwrap();
        let tree = build_section_tree(sections);

        // s1: no parent, no prev, next=s2
        assert_eq!(tree[0].parent_anchor, None);
        assert_eq!(tree[0].prev_anchor, None);
        assert_eq!(tree[0].next_anchor, Some("s2".to_string()));

        // s1-1: parent=s1, no prev, next=s1-2
        assert_eq!(tree[1].parent_anchor, Some("s1".to_string()));
        assert_eq!(tree[1].prev_anchor, None);
        assert_eq!(tree[1].next_anchor, Some("s1-2".to_string()));

        // s1-2: parent=s1, prev=s1-1, no next
        assert_eq!(tree[2].parent_anchor, Some("s1".to_string()));
        assert_eq!(tree[2].prev_anchor, Some("s1-1".to_string()));
        assert_eq!(tree[2].next_anchor, None);

        // s1-2-1: parent=s1-2, no prev, no next
        assert_eq!(tree[3].parent_anchor, Some("s1-2".to_string()));
        assert_eq!(tree[3].prev_anchor, None);
        assert_eq!(tree[3].next_anchor, None);

        // s2: no parent, prev=s1, no next
        assert_eq!(tree[4].parent_anchor, None);
        assert_eq!(tree[4].prev_anchor, Some("s1".to_string()));
        assert_eq!(tree[4].next_anchor, None);
    }

    #[test]
    fn test_build_section_tree_flat_structure() {
        let html = r#"
            <h2 id="a">A</h2>
            <h2 id="b">B</h2>
            <h2 id="c">C</h2>
        "#;

        let sections = collect_headings(html).unwrap();
        let tree = build_section_tree(sections);

        // a: no parent, no prev, next=b
        assert_eq!(tree[0].parent_anchor, None);
        assert_eq!(tree[0].prev_anchor, None);
        assert_eq!(tree[0].next_anchor, Some("b".to_string()));

        // b: no parent, prev=a, next=c
        assert_eq!(tree[1].parent_anchor, None);
        assert_eq!(tree[1].prev_anchor, Some("a".to_string()));
        assert_eq!(tree[1].next_anchor, Some("c".to_string()));

        // c: no parent, prev=b, no next
        assert_eq!(tree[2].parent_anchor, None);
        assert_eq!(tree[2].prev_anchor, Some("b".to_string()));
        assert_eq!(tree[2].next_anchor, None);
    }

    #[test]
    fn test_build_section_tree_single_heading() {
        let html = r#"<h2 id="only">Only Section</h2>"#;

        let sections = collect_headings(html).unwrap();
        let tree = build_section_tree(sections);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].parent_anchor, None);
        assert_eq!(tree[0].prev_anchor, None);
        assert_eq!(tree[0].next_anchor, None);
    }

    #[test]
    fn test_build_section_tree_skip_levels() {
        // Test when heading levels are skipped (h2 -> h4, skipping h3)
        let html = r#"
            <h2 id="top">Top</h2>
            <h4 id="nested">Nested (skipped h3)</h4>
            <h2 id="next">Next Top</h2>
        "#;

        let sections = collect_headings(html).unwrap();
        let tree = build_section_tree(sections);

        // nested: parent should still be 'top' (nearest lower depth)
        assert_eq!(tree[1].parent_anchor, Some("top".to_string()));
        assert_eq!(tree[1].prev_anchor, None); // no siblings at depth 4
        assert_eq!(tree[1].next_anchor, None);
    }

    #[test]
    fn test_bikeshed_definition_parsing() {
        let html = include_str!("../../tests/fixtures/definitions/bikeshed_definition.html");
        let sections = collect_definitions(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "concept-tree");
        assert_eq!(section.title, Some("tree".to_string()));
        assert_eq!(section.section_type, SectionType::Definition);
        assert_eq!(section.depth, None);
    }

    #[test]
    fn test_wattsi_definition_parsing() {
        let html = include_str!("../../tests/fixtures/definitions/wattsi_definition.html");
        let sections = collect_definitions(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "in-parallel");
        assert_eq!(section.title, Some("in parallel".to_string()));
        assert_eq!(section.section_type, SectionType::Definition);
        assert_eq!(section.depth, None);
    }

    #[test]
    fn test_definition_with_code() {
        let html = include_str!("../../tests/fixtures/definitions/definition_with_code.html");
        let sections = collect_definitions(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "x-that");
        assert_eq!(section.title, Some("createElement".to_string()));
        assert_eq!(section.section_type, SectionType::Definition);
    }

    #[test]
    fn test_definition_without_id_ignored() {
        let html = r#"
            <dfn id="has-id">With ID</dfn>
            <dfn>Without ID</dfn>
        "#;

        let sections = collect_definitions(html).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].anchor, "has-id");
    }

    #[test]
    fn test_multiple_definitions() {
        let html = r#"
            <p>A <dfn id="def-1">first term</dfn> and a <dfn id="def-2">second term</dfn>.</p>
            <p>Also a <dfn id="def-3">third term</dfn>.</p>
        "#;

        let sections = collect_definitions(html).unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].anchor, "def-1");
        assert_eq!(sections[1].anchor, "def-2");
        assert_eq!(sections[2].anchor, "def-3");
    }

    #[test]
    fn test_bikeshed_algorithm_parsing() {
        let html = include_str!("../../tests/fixtures/algorithms/bikeshed_algorithm.html");
        let sections = collect_algorithms(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "concept-ordered-set-parser");
        assert_eq!(section.title, Some("ordered set parser".to_string()));
        assert_eq!(section.section_type, SectionType::Algorithm);
        assert_eq!(section.depth, None);
    }

    #[test]
    fn test_algorithm_vs_definition_distinction() {
        let html =
            include_str!("../../tests/fixtures/algorithms/mixed_definitions_algorithms.html");

        // Collect algorithms (dfn inside div.algorithm)
        let algorithms = collect_algorithms(html).unwrap();
        assert_eq!(algorithms.len(), 1);
        assert_eq!(algorithms[0].anchor, "algorithm-def");
        assert_eq!(algorithms[0].section_type, SectionType::Algorithm);

        // Collect definitions (dfn NOT inside div.algorithm)
        let definitions = collect_definitions(html).unwrap();
        assert_eq!(definitions.len(), 2);
        assert_eq!(definitions[0].anchor, "standalone-def");
        assert_eq!(definitions[0].section_type, SectionType::Definition);
        assert_eq!(definitions[1].anchor, "another-standalone");
        assert_eq!(definitions[1].section_type, SectionType::Definition);

        // No overlap: the dfn inside algorithm div should not appear in definitions
        let def_anchors: Vec<_> = definitions.iter().map(|d| &d.anchor).collect();
        assert!(!def_anchors.contains(&&"algorithm-def".to_string()));
    }

    #[test]
    fn test_algorithm_without_dfn() {
        // Some algorithms might not have a dfn, just the algorithm div
        let html = r#"
            <div class="algorithm" data-algorithm="no dfn">
                <p>This algorithm has no dfn element.</p>
                <ol><li>Step 1</li></ol>
            </div>
        "#;

        let sections = collect_algorithms(html).unwrap();
        assert_eq!(sections.len(), 0); // No dfn[id], so nothing to index
    }

    #[test]
    fn test_idl_interface_parsing() {
        let html = include_str!("../../tests/fixtures/idl/interface.html");
        let sections = collect_idl(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "event");
        assert_eq!(section.title, Some("Event".to_string()));
        assert_eq!(section.section_type, SectionType::Idl);
        assert_eq!(section.depth, None);
    }

    #[test]
    fn test_idl_dictionary_parsing() {
        let html = include_str!("../../tests/fixtures/idl/dictionary.html");
        let sections = collect_idl(html).unwrap();

        assert_eq!(sections.len(), 1);
        let section = &sections[0];

        assert_eq!(section.anchor, "eventinit");
        assert_eq!(section.title, Some("EventInit".to_string()));
        assert_eq!(section.section_type, SectionType::Idl);
        assert_eq!(section.depth, None);
    }

    #[test]
    fn test_idl_vs_definition_distinction() {
        let html = include_str!("../../tests/fixtures/idl/mixed_idl_definitions.html");

        // Collect IDL types (dfn with data-dfn-type="interface", "dictionary", etc.)
        let idl = collect_idl(html).unwrap();
        assert_eq!(idl.len(), 2);
        assert_eq!(idl[0].anchor, "myinterface");
        assert_eq!(idl[0].section_type, SectionType::Idl);
        assert_eq!(idl[1].anchor, "mydict");
        assert_eq!(idl[1].section_type, SectionType::Idl);

        // Collect definitions (dfn NOT IDL types and NOT in algorithm divs)
        let definitions = collect_definitions(html).unwrap();
        assert_eq!(definitions.len(), 2);
        assert_eq!(definitions[0].anchor, "regular-term");
        assert_eq!(definitions[0].section_type, SectionType::Definition);
        assert_eq!(definitions[1].anchor, "another-term");
        assert_eq!(definitions[1].section_type, SectionType::Definition);

        // No overlap: IDL types should not appear in definitions
        let def_anchors: Vec<_> = definitions.iter().map(|d| &d.anchor).collect();
        assert!(!def_anchors.contains(&&"myinterface".to_string()));
        assert!(!def_anchors.contains(&&"mydict".to_string()));
    }

    #[test]
    fn test_idl_without_data_dfn_type_ignored() {
        let html = r#"
            <pre class="idl">
                <dfn id="has-type" data-dfn-type="interface">WithType</dfn>
                <dfn id="no-type">WithoutType</dfn>
            </pre>
        "#;

        let sections = collect_idl(html).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].anchor, "has-type");
    }

    #[test]
    fn test_wattsi_algorithm_pattern() {
        // Test Wattsi-style algorithm: <p>To <dfn>foo</dfn>:</p><ol>...</ol>
        // (as opposed to Bikeshed's <div class="algorithm"><p>To <dfn>foo</dfn>:</p><ol>...</ol></div>)
        let html = include_str!("../../tests/fixtures/algorithms/wattsi_navigate.html");
        let converter = crate::parse::markdown::build_converter("https://html.spec.whatwg.org");

        let document = Html::parse_document(html);
        let selector = Selector::parse("dfn[id]").unwrap();

        let mut algorithms = Vec::new();
        for element in document.select(&selector) {
            if let Some(section) = parse_dfn_element(&element, &converter).unwrap() {
                algorithms.push(section);
            }
        }

        assert_eq!(algorithms.len(), 1, "Should detect one algorithm");
        let algo = &algorithms[0];

        assert_eq!(algo.anchor, "navigate");
        assert_eq!(algo.title, Some("navigate".to_string()));
        assert_eq!(
            algo.section_type,
            SectionType::Algorithm,
            "Should be classified as Algorithm, not Definition"
        );

        // Check that content includes both intro and steps (now markdown)
        let content = algo.content_text.as_ref().unwrap();
        assert!(content.contains("navigate"), "Should include intro text");
        assert!(content.contains("1. "), "Should include first step");
        assert!(content.contains("2. "), "Should include second step");
        // Check for nested step (step 4 has sub-steps in the fixture)
        assert!(
            content.contains("    1. "),
            "Should include nested step with indentation"
        );
    }

    #[test]
    fn test_dfn_inside_algorithm_content_skipped() {
        // Dfns that appear inside algorithm <ol> content should NOT be collected as separate sections
        // They're part of the algorithm's markdown content
        let html = r#"
            <h2 id="algorithms">Algorithms</h2>
            <p>To <dfn id="do-something">do something</dfn> with <var>input</var>:</p>
            <ol>
                <li><p>Let <var>result</var> be the result of calling <dfn id="helper">helper</dfn>.</p></li>
                <li><p>Return <var>result</var>.</p></li>
            </ol>
            <p>The <dfn id="outside-def">outside definition</dfn> is separate.</p>
        "#;

        let converter = crate::parse::markdown::build_converter("https://test.example.com");
        let document = Html::parse_document(html);
        let selector = Selector::parse("dfn[id]").unwrap();

        let mut sections = Vec::new();
        for element in document.select(&selector) {
            if let Some(section) = parse_dfn_element(&element, &converter).unwrap() {
                sections.push(section);
            }
        }

        // Should only collect "do-something" (the algorithm) and "outside-def"
        // "helper" inside the <ol> should be skipped
        assert_eq!(
            sections.len(),
            2,
            "Should collect 2 sections (algorithm + outside def), not the helper inside <ol>"
        );

        let anchors: Vec<_> = sections.iter().map(|s| s.anchor.as_str()).collect();
        assert!(
            anchors.contains(&"do-something"),
            "Should include the algorithm-defining dfn"
        );
        assert!(
            anchors.contains(&"outside-def"),
            "Should include the outside definition"
        );
        assert!(
            !anchors.contains(&"helper"),
            "Should NOT include dfn inside algorithm <ol>"
        );
    }

    #[test]
    fn test_dfn_inside_bikeshed_algorithm_content_skipped() {
        // Same test but for Bikeshed div.algorithm pattern
        let html = r#"
            <h2 id="algorithms">Algorithms</h2>
            <div class="algorithm">
                <p>To <dfn id="process">process</dfn> the <var>data</var>:</p>
                <ol>
                    <li><p>Let <var>x</var> be a new <dfn id="internal-thing">internal thing</dfn>.</p></li>
                    <li><p>Return <var>x</var>.</p></li>
                </ol>
            </div>
            <p>A <dfn id="external-term">external term</dfn> here.</p>
        "#;

        let converter = crate::parse::markdown::build_converter("https://test.example.com");
        let document = Html::parse_document(html);
        let selector = Selector::parse("dfn[id]").unwrap();

        let mut sections = Vec::new();
        for element in document.select(&selector) {
            if let Some(section) = parse_dfn_element(&element, &converter).unwrap() {
                sections.push(section);
            }
        }

        // Should only collect "process" (the algorithm) and "external-term"
        assert_eq!(
            sections.len(),
            2,
            "Should collect 2 sections, not the internal-thing inside <ol>"
        );

        let anchors: Vec<_> = sections.iter().map(|s| s.anchor.as_str()).collect();
        assert!(anchors.contains(&"process"));
        assert!(anchors.contains(&"external-term"));
        assert!(
            !anchors.contains(&"internal-thing"),
            "Should NOT include dfn inside algorithm <ol>"
        );
    }

    #[test]
    fn test_parameter_dfns_skipped() {
        // Parameter dfns (with data-dfn-for or containing <var>) should NOT be collected as sections
        // They're part of the parent definition/algorithm signature, not standalone sections
        let html = r#"
            <h2 id="algorithms">Algorithms</h2>
            <p>To <dfn id="navigate">navigate</dfn> with <dfn data-dfn-for="navigate" id="param1"><var>url</var></dfn>
            and <dfn id="param2"><var>options</var></dfn>:</p>
            <ol>
                <li><p>Do something.</p></li>
            </ol>
            <p>A standalone <dfn id="regular-def">definition</dfn>.</p>
        "#;

        let converter = crate::parse::markdown::build_converter("https://test.example.com");
        let document = Html::parse_document(html);
        let selector = Selector::parse("dfn[id]").unwrap();

        let mut sections = Vec::new();
        for element in document.select(&selector) {
            if let Some(section) = parse_dfn_element(&element, &converter).unwrap() {
                sections.push(section);
            }
        }

        // Should only collect "navigate" (algorithm) and "regular-def" (standalone definition)
        // Parameter dfns "param1" (has data-dfn-for) and "param2" (contains <var>) should be skipped
        assert_eq!(
            sections.len(),
            2,
            "Should collect 2 sections (algorithm + regular def)"
        );

        let anchors: Vec<_> = sections.iter().map(|s| s.anchor.as_str()).collect();
        assert!(
            anchors.contains(&"navigate"),
            "Should include the algorithm"
        );
        assert!(
            anchors.contains(&"regular-def"),
            "Should include standalone definition"
        );
        assert!(
            !anchors.contains(&"param1"),
            "Should NOT include parameter dfn with data-dfn-for"
        );
        assert!(
            !anchors.contains(&"param2"),
            "Should NOT include parameter dfn containing <var>"
        );
    }

    #[test]
    fn test_property_dfns_with_dfn_for_and_dfn_type_kept() {
        // dfns with data-dfn-for AND data-dfn-type="dfn" are property definitions,
        // not parameters. They should be indexed.
        // Real example from DOM spec: <dfn data-dfn-for="tree" data-dfn-type="dfn" id="concept-tree-parent">parent</dfn>
        let html = r#"
            <h2 id="trees">Trees</h2>
            <p>An object that <dfn class="dfn-paneled" data-dfn-type="dfn" data-export id="concept-tree">participates</dfn>
            in a tree has a <dfn class="dfn-paneled" data-dfn-for="tree" data-dfn-type="dfn" data-export id="concept-tree-parent">parent</dfn>,
            which is either null or an object, and has
            <dfn class="dfn-paneled" data-dfn-for="tree" data-dfn-type="dfn" data-export id="concept-tree-child">children</dfn>,
            which is an ordered set of objects.</p>
        "#;

        let converter = crate::parse::markdown::build_converter("https://test.example.com");
        let document = Html::parse_document(html);
        let selector = Selector::parse("dfn[id]").unwrap();

        let mut sections = Vec::new();
        for element in document.select(&selector) {
            if let Some(section) = parse_dfn_element(&element, &converter).unwrap() {
                sections.push(section);
            }
        }

        let anchors: Vec<_> = sections.iter().map(|s| s.anchor.as_str()).collect();
        assert!(
            anchors.contains(&"concept-tree"),
            "Should include dfn without data-dfn-for"
        );
        assert!(
            anchors.contains(&"concept-tree-parent"),
            "Should include property dfn with data-dfn-for + data-dfn-type"
        );
        assert!(
            anchors.contains(&"concept-tree-child"),
            "Should include property dfn with data-dfn-for + data-dfn-type"
        );
    }

    #[test]
    fn test_argument_dfns_skipped() {
        // Bikeshed-generated W3C specs use data-dfn-type="argument" for function parameters.
        // These should be skipped, while method/attribute/interface/constructor dfns are kept.
        let html = r#"
            <h2 id="api">API</h2>
            <pre class="idl">
                <dfn data-dfn-type="interface" id="audiodecoder"><code>AudioDecoder</code></dfn>
                <dfn data-dfn-for="AudioDecoder" data-dfn-type="constructor" id="dom-audiodecoder-ctor"><code>AudioDecoder(init)</code></dfn>
                <dfn data-dfn-for="AudioDecoder/AudioDecoder(init)" data-dfn-type="argument" id="dom-audiodecoder-ctor-init"><code>init</code></dfn>
                <dfn data-dfn-for="AudioDecoder" data-dfn-type="method" id="dom-audiodecoder-configure"><code>configure(config)</code></dfn>
                <dfn data-dfn-for="AudioDecoder/configure(config)" data-dfn-type="argument" id="dom-audiodecoder-configure-config"><code>config</code></dfn>
                <dfn data-dfn-for="AudioDecoder" data-dfn-type="attribute" id="dom-audiodecoder-state"><code>state</code></dfn>
            </pre>
        "#;

        let converter = crate::parse::markdown::build_converter("https://test.example.com");
        let document = Html::parse_document(html);
        let selector = Selector::parse("dfn[id]").unwrap();

        let mut sections = Vec::new();
        for element in document.select(&selector) {
            if let Some(section) = parse_dfn_element(&element, &converter).unwrap() {
                sections.push(section);
            }
        }

        let anchors: Vec<_> = sections.iter().map(|s| s.anchor.as_str()).collect();

        // Interface, constructor, method, attribute should be kept
        assert!(anchors.contains(&"audiodecoder"), "Interface should be kept");
        assert!(
            anchors.contains(&"dom-audiodecoder-ctor"),
            "Constructor should be kept"
        );
        assert!(
            anchors.contains(&"dom-audiodecoder-configure"),
            "Method should be kept"
        );
        assert!(
            anchors.contains(&"dom-audiodecoder-state"),
            "Attribute should be kept"
        );

        // Arguments should be skipped
        assert!(
            !anchors.contains(&"dom-audiodecoder-ctor-init"),
            "Argument should be skipped"
        );
        assert!(
            !anchors.contains(&"dom-audiodecoder-configure-config"),
            "Argument should be skipped"
        );
    }
}
