//! Markdown output formatters for CLI commands

use crate::model::{AnchorsResult, ExistsResult, ListEntry, QueryResult, RefsResult, SearchResult};

#[cfg(test)]
use crate::model::{AnchorEntry, SearchEntry};

/// Format a QueryResult as markdown
pub fn query(result: &QueryResult) -> String {
    let mut md = String::new();

    md.push_str(&format!("# {}#{}\n\n", result.spec, result.anchor));

    if let Some(title) = &result.title {
        md.push_str(&format!("**{}** ({})\n\n", title, result.section_type));
    } else {
        md.push_str(&format!("**Type**: {}\n\n", result.section_type));
    }

    md.push_str(&format!("**SHA**: {}\n\n", result.sha));

    if let Some(content) = &result.content {
        md.push_str("## Content\n\n");
        md.push_str(content);
        md.push_str("\n\n");
    }

    // Navigation
    md.push_str("## Navigation\n\n");
    if let Some(parent) = &result.navigation.parent {
        md.push_str(&format!(
            "- Parent: `{}`{}\n",
            parent.anchor,
            parent
                .title
                .as_deref()
                .map_or(String::new(), |t| format!(" — {}", t))
        ));
    }
    if let Some(prev) = &result.navigation.prev {
        md.push_str(&format!(
            "- Prev: `{}`{}\n",
            prev.anchor,
            prev.title
                .as_deref()
                .map_or(String::new(), |t| format!(" — {}", t))
        ));
    }
    if let Some(next) = &result.navigation.next {
        md.push_str(&format!(
            "- Next: `{}`{}\n",
            next.anchor,
            next.title
                .as_deref()
                .map_or(String::new(), |t| format!(" — {}", t))
        ));
    }
    if !result.navigation.children.is_empty() {
        md.push_str(&format!("- Children: {}\n", result.navigation.children.len()));
        for child in &result.navigation.children {
            md.push_str(&format!(
                "  - `{}`{}\n",
                child.anchor,
                child
                    .title
                    .as_deref()
                    .map_or(String::new(), |t| format!(" — {}", t))
            ));
        }
    }

    if !result.outgoing_refs.is_empty() {
        md.push_str(&format!(
            "\n## Outgoing refs ({})\n\n",
            result.outgoing_refs.len()
        ));
        for ref_entry in &result.outgoing_refs {
            md.push_str(&format!("- {}#{}\n", ref_entry.spec, ref_entry.anchor));
        }
    }

    if !result.incoming_refs.is_empty() {
        md.push_str(&format!(
            "\n## Incoming refs ({})\n\n",
            result.incoming_refs.len()
        ));
        for ref_entry in &result.incoming_refs {
            md.push_str(&format!("- {}#{}\n", ref_entry.spec, ref_entry.anchor));
        }
    }

    md
}

/// Format an ExistsResult as markdown
pub fn exists(result: &ExistsResult) -> String {
    if result.exists {
        format!(
            "{}#{} exists ({})\n",
            result.spec,
            result.anchor,
            result.section_type.as_deref().unwrap_or("unknown")
        )
    } else {
        format!("{}#{} not found\n", result.spec, result.anchor)
    }
}

/// Format an AnchorsResult as markdown
pub fn anchors(result: &AnchorsResult) -> String {
    let mut md = String::new();

    md.push_str(&format!(
        "# Anchors matching `{}`\n\n",
        result.pattern
    ));

    if result.results.is_empty() {
        md.push_str("No results.\n");
    } else {
        for entry in &result.results {
            md.push_str(&format!(
                "- **{}#{}**{} ({})\n",
                entry.spec,
                entry.anchor,
                entry
                    .title
                    .as_deref()
                    .map_or(String::new(), |t| format!(" — {}", t)),
                entry.section_type,
            ));
        }
    }

    md
}

/// Format a SearchResult as markdown
pub fn search(result: &SearchResult) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Search: \"{}\"\n\n", result.query));

    if result.results.is_empty() {
        md.push_str("No results.\n");
    } else {
        for entry in &result.results {
            md.push_str(&format!(
                "### {}#{}{}\n\n",
                entry.spec,
                entry.anchor,
                entry
                    .title
                    .as_deref()
                    .map_or(String::new(), |t| format!(" — {}", t)),
            ));
            if !entry.snippet.is_empty() {
                md.push_str(&format!("{}\n\n", entry.snippet));
            }
        }
    }

    md
}

/// Format a list of headings as markdown (tree structure)
pub fn list(entries: &[ListEntry]) -> String {
    let mut md = String::new();

    for entry in entries {
        let indent = if entry.depth > 2 {
            "  ".repeat((entry.depth - 2) as usize)
        } else {
            String::new()
        };

        md.push_str(&format!(
            "{}- `{}`{}\n",
            indent,
            entry.anchor,
            entry
                .title
                .as_deref()
                .map_or(String::new(), |t| format!(" — {}", t)),
        ));
    }

    md
}

/// Format a RefsResult as markdown
pub fn refs(result: &RefsResult) -> String {
    let mut md = String::new();

    md.push_str(&format!("# Refs for `{}`\n\n", result.anchor));

    if let Some(outgoing) = &result.outgoing {
        md.push_str(&format!("## Outgoing ({})\n\n", outgoing.len()));
        for ref_entry in outgoing {
            md.push_str(&format!("- {}#{}\n", ref_entry.spec, ref_entry.anchor));
        }
        md.push('\n');
    }

    if let Some(incoming) = &result.incoming {
        md.push_str(&format!("## Incoming ({})\n\n", incoming.len()));
        for ref_entry in incoming {
            md.push_str(&format!("- {}#{}\n", ref_entry.spec, ref_entry.anchor));
        }
    }

    if result.outgoing.is_none() && result.incoming.is_none() {
        md.push_str("No references found\n");
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NavEntry, Navigation, RefEntry};

    #[test]
    fn test_query_format_minimal() {
        let result = QueryResult {
            spec: "TEST".to_string(),
            sha: "abc123".to_string(),
            anchor: "test-section".to_string(),
            title: None,
            content: None,
            section_type: "Heading".to_string(),
            navigation: Navigation {
                parent: None,
                prev: None,
                next: None,
                children: vec![],
            },
            outgoing_refs: vec![],
            incoming_refs: vec![],
        };

        let md = query(&result);
        assert!(md.contains("# TEST#test-section"));
        assert!(md.contains("**Type**: Heading"));
        assert!(md.contains("**SHA**: abc123"));
        assert!(md.contains("## Navigation"));
    }

    #[test]
    fn test_query_format_with_content() {
        let result = QueryResult {
            spec: "TEST".to_string(),
            sha: "abc123".to_string(),
            anchor: "navigate".to_string(),
            title: Some("navigate".to_string()),
            content: Some("To **navigate** a [navigable](#foo)".to_string()),
            section_type: "Algorithm".to_string(),
            navigation: Navigation {
                parent: Some(NavEntry {
                    anchor: "section-7".to_string(),
                    title: None,
                }),
                prev: None,
                next: None,
                children: vec![],
            },
            outgoing_refs: vec![],
            incoming_refs: vec![],
        };

        let md = query(&result);
        assert!(md.contains("**navigate** (Algorithm)"));
        assert!(md.contains("## Content"));
        assert!(md.contains("To **navigate** a [navigable](#foo)"));
        assert!(md.contains("- Parent: `section-7`"));
    }

    #[test]
    fn test_query_format_with_refs() {
        let result = QueryResult {
            spec: "TEST".to_string(),
            sha: "abc123".to_string(),
            anchor: "foo".to_string(),
            title: None,
            content: None,
            section_type: "Definition".to_string(),
            navigation: Navigation {
                parent: None,
                prev: None,
                next: None,
                children: vec![
                    NavEntry {
                        anchor: "child1".to_string(),
                        title: Some("First Child".to_string()),
                    },
                    NavEntry {
                        anchor: "child2".to_string(),
                        title: None,
                    },
                ],
            },
            outgoing_refs: vec![RefEntry {
                spec: "OTHER".to_string(),
                anchor: "bar".to_string(),
            }],
            incoming_refs: vec![RefEntry {
                spec: "ANOTHER".to_string(),
                anchor: "baz".to_string(),
            }],
        };

        let md = query(&result);
        assert!(md.contains("- Children: 2"));
        assert!(md.contains("  - `child1` — First Child"));
        assert!(md.contains("  - `child2`"));
        assert!(md.contains("## Outgoing refs (1)"));
        assert!(md.contains("- OTHER#bar"));
        assert!(md.contains("## Incoming refs (1)"));
        assert!(md.contains("- ANOTHER#baz"));
    }

    #[test]
    fn test_exists_true() {
        let result = ExistsResult {
            exists: true,
            spec: "HTML".to_string(),
            anchor: "navigate".to_string(),
            section_type: Some("Algorithm".to_string()),
        };
        let md = exists(&result);
        assert_eq!(md, "HTML#navigate exists (Algorithm)\n");
    }

    #[test]
    fn test_exists_false() {
        let result = ExistsResult {
            exists: false,
            spec: "DOM".to_string(),
            anchor: "missing".to_string(),
            section_type: None,
        };
        let md = exists(&result);
        assert_eq!(md, "DOM#missing not found\n");
    }

    #[test]
    fn test_anchors_format() {
        let result = AnchorsResult {
            pattern: "*-tree".to_string(),
            results: vec![
                AnchorEntry {
                    spec: "DOM".to_string(),
                    anchor: "concept-tree".to_string(),
                    title: Some("tree".to_string()),
                    section_type: "Definition".to_string(),
                },
                AnchorEntry {
                    spec: "HTML".to_string(),
                    anchor: "document-tree".to_string(),
                    title: None,
                    section_type: "Definition".to_string(),
                },
            ],
        };

        let md = anchors(&result);
        assert!(md.contains("# Anchors matching `*-tree`"));
        assert!(md.contains("- **DOM#concept-tree** — tree (Definition)"));
        assert!(md.contains("- **HTML#document-tree** (Definition)"));
    }

    #[test]
    fn test_search_format() {
        let result = SearchResult {
            query: "tree order".to_string(),
            results: vec![SearchEntry {
                spec: "DOM".to_string(),
                anchor: "concept-tree-order".to_string(),
                title: Some("tree order".to_string()),
                section_type: "Definition".to_string(),
                snippet: "An object A is before an object B in <mark>tree order</mark>...".to_string(),
            }],
        };

        let md = search(&result);
        assert!(md.contains("# Search: \"tree order\""));
        assert!(md.contains("### DOM#concept-tree-order — tree order"));
        assert!(md.contains("An object A is before"));
    }

    #[test]
    fn test_list_format() {
        let entries = vec![
            ListEntry {
                anchor: "intro".to_string(),
                title: Some("Introduction".to_string()),
                depth: 2,
                parent: None,
            },
            ListEntry {
                anchor: "algorithms".to_string(),
                title: Some("Algorithms".to_string()),
                depth: 3,
                parent: Some("intro".to_string()),
            },
        ];

        let md = list(&entries);
        assert!(md.contains("- `intro` — Introduction"));
        assert!(md.contains("  - `algorithms` — Algorithms")); // depth 3 gets 1 level indent
    }

    #[test]
    fn test_list_format_empty() {
        let md = list(&[]);
        assert_eq!(md, "");
    }

    #[test]
    fn test_refs_format_both_directions() {
        let result = RefsResult {
            anchor: "navigate".to_string(),
            direction: "both".to_string(),
            outgoing: Some(vec![
                RefEntry {
                    spec: "URL".to_string(),
                    anchor: "concept-url".to_string(),
                },
                RefEntry {
                    spec: "INFRA".to_string(),
                    anchor: "assert".to_string(),
                },
            ]),
            incoming: Some(vec![RefEntry {
                spec: "HTML".to_string(),
                anchor: "navigate-fragid".to_string(),
            }]),
        };

        let md = refs(&result);
        assert!(md.contains("# Refs for `navigate`"));
        assert!(md.contains("## Outgoing (2)"));
        assert!(md.contains("- URL#concept-url"));
        assert!(md.contains("- INFRA#assert"));
        assert!(md.contains("## Incoming (1)"));
        assert!(md.contains("- HTML#navigate-fragid"));
    }

    #[test]
    fn test_refs_format_no_refs() {
        let result = RefsResult {
            anchor: "orphan".to_string(),
            direction: "both".to_string(),
            outgoing: None,
            incoming: None,
        };

        let md = refs(&result);
        assert!(md.contains("# Refs for `orphan`"));
        assert!(md.contains("No references found"));
    }
}
