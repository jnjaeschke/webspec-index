//! Hover content formatting for the LSP server.

use crate::model::QueryResult;

/// Format a query result as markdown for a hover tooltip.
pub fn build_hover_content(result: &QueryResult) -> String {
    let mut parts = Vec::new();

    let heading = result
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .or(Some(&result.anchor));
    if let Some(h) = heading {
        parts.push(format!("## {h}"));
    }

    if !result.section_type.is_empty() {
        parts.push(format!(
            "*{}* | {}#{}",
            result.section_type, result.spec, result.anchor
        ));
    }

    if let Some(content) = &result.content {
        if !content.is_empty() {
            parts.push(content.clone());
        }
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Navigation, QueryResult};

    fn make_result(title: Option<&str>, section_type: &str, content: Option<&str>) -> QueryResult {
        QueryResult {
            spec: "HTML".to_string(),
            sha: "abc".to_string(),
            anchor: "navigate".to_string(),
            title: title.map(|s| s.to_string()),
            section_type: section_type.to_string(),
            content: content.map(|s| s.to_string()),
            navigation: Navigation {
                parent: None,
                prev: None,
                next: None,
                children: vec![],
            },
            outgoing_refs: vec![],
            incoming_refs: vec![],
        }
    }

    #[test]
    fn full_result() {
        let result = make_result(
            Some("navigate"),
            "Algorithm",
            Some("To **navigate** a navigable..."),
        );
        let md = build_hover_content(&result);
        assert!(md.contains("## navigate"));
        assert!(md.contains("*Algorithm*"));
        assert!(md.contains("HTML#navigate"));
        assert!(md.contains("To **navigate**"));
    }

    #[test]
    fn minimal_result() {
        let mut result = make_result(None, "", None);
        result.anchor = "some-section".to_string();
        let md = build_hover_content(&result);
        assert!(md.contains("some-section"));
    }

    #[test]
    fn no_content() {
        let mut result = make_result(Some("Trees"), "Heading", None);
        result.spec = "DOM".to_string();
        result.anchor = "concept-tree".to_string();
        let md = build_hover_content(&result);
        assert!(md.contains("## Trees"));
        assert!(md.contains("*Heading*"));
    }

    #[test]
    fn title_fallback_to_anchor() {
        let mut result = make_result(Some(""), "", Some("Some content here."));
        result.anchor = "my-anchor".to_string();
        let md = build_hover_content(&result);
        assert!(md.contains("my-anchor"));
    }
}
