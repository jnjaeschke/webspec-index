//! Markdown output formatters for CLI commands

use crate::model::{
    AnchorsResult, ExistsResult, GraphResult, IdlResult, ListEntry, QueryResult, RefsResult,
    SearchResult,
};

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
        md.push_str(&format!(
            "- Children: {}\n",
            result.navigation.children.len()
        ));
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

    md.push_str(&format!("# Anchors matching `{}`\n\n", result.pattern));

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
    md.push_str(&format!(
        "# refs: `{}` ({})\n\n",
        result.query, result.direction
    ));

    if result.matches.is_empty() {
        md.push_str("No matches found in indexed specs.\n");
        return md;
    }

    for m in &result.matches {
        md.push_str(&format!(
            "## {}#{} ({}, {})\n\n",
            m.spec, m.anchor, m.section_type, m.resolution
        ));
        if let Some(title) = &m.title {
            md.push_str(&format!("Title: **{}**\n\n", title));
        }

        if let Some(incoming) = &m.incoming {
            md.push_str(&format!("Incoming: {}\n", incoming.len()));
            for r in incoming {
                md.push_str(&format!("- {}#{}\n", r.spec, r.anchor));
            }
            md.push('\n');
        }

        if let Some(outgoing) = &m.outgoing {
            md.push_str(&format!("Outgoing: {}\n", outgoing.len()));
            for r in outgoing {
                md.push_str(&format!("- {}#{}\n", r.spec, r.anchor));
            }
            md.push('\n');
        }
    }

    md
}

/// Format a GraphResult as markdown
pub fn graph(result: &GraphResult) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "# graph {}#{} ({})\n\n",
        result.root.spec, result.root.anchor, result.direction
    ));
    md.push_str(&format!(
        "Nodes: {} | Edges: {} | Max depth: {} | Truncated: {}\n\n",
        result.nodes.len(),
        result.edges.len(),
        result.max_depth,
        result.truncated
    ));

    md.push_str("## Nodes\n\n");
    for node in &result.nodes {
        md.push_str(&format!(
            "- `{}`{}\n",
            node.id,
            node.title
                .as_deref()
                .map_or(String::new(), |t| format!(" — {}", t))
        ));
    }

    md.push_str("\n## Edges\n\n");
    for edge in &result.edges {
        md.push_str(&format!(
            "- `{}` -> `{}` ({})\n",
            edge.from, edge.to, edge.kind
        ));
    }

    md
}

fn escape_mermaid_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_dot_label(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Render a GraphResult as Mermaid flowchart.
pub fn graph_mermaid(result: &GraphResult) -> String {
    let mut out = String::from("graph TD\n");
    let mut ids = std::collections::HashMap::new();
    let mut bridge_nodes = Vec::new();
    let mut root_nodes = Vec::new();

    for (idx, node) in result.nodes.iter().enumerate() {
        let local_id = format!("n{}", idx);
        ids.insert(node.id.clone(), local_id.clone());
        let label = if let Some(title) = &node.title {
            format!("{}<br>{}", node.id, title.replace('\n', "<br>"))
        } else {
            node.id.clone()
        };
        out.push_str(&format!(
            "  {}[\"{}\"]\n",
            local_id,
            escape_mermaid_label(&label)
        ));

        if let Some(role) = &node.filter_role {
            match role.as_str() {
                "bridge" => bridge_nodes.push(local_id.clone()),
                "root" => root_nodes.push(local_id.clone()),
                _ => {}
            }
        }
    }

    for edge in &result.edges {
        if let (Some(from), Some(to)) = (ids.get(&edge.from), ids.get(&edge.to)) {
            out.push_str(&format!("  {} --> {}\n", from, to));
        }
    }

    if !bridge_nodes.is_empty() {
        out.push_str("  classDef bridge stroke-dasharray: 5 5\n");
        out.push_str(&format!("  class {} bridge\n", bridge_nodes.join(",")));
    }
    if !root_nodes.is_empty() {
        out.push_str("  classDef root stroke-width: 3px\n");
        out.push_str(&format!("  class {} root\n", root_nodes.join(",")));
    }

    out
}

/// Render a GraphResult as Graphviz DOT.
pub fn graph_dot(result: &GraphResult) -> String {
    let mut out = String::from("digraph webspec {\n  rankdir=LR;\n");

    for node in &result.nodes {
        let escaped_id = escape_dot_label(&node.id);
        let label = if let Some(title) = &node.title {
            format!("{}\\n{}", escaped_id, escape_dot_label(title))
        } else {
            escaped_id.clone()
        };
        out.push_str(&format!("  \"{}\" [label=\"{}\"];\n", escaped_id, label));
    }

    for edge in &result.edges {
        out.push_str(&format!(
            "  \"{}\" -> \"{}\";\n",
            escape_dot_label(&edge.from),
            escape_dot_label(&edge.to)
        ));
    }

    out.push_str("}\n");
    out
}

/// Format an IdlResult as markdown
pub fn idl(result: &IdlResult) -> String {
    let mut md = String::new();
    md.push_str(&format!("# IDL: `{}`\n\n", result.query));

    if result.matches.is_empty() {
        md.push_str("No IDL matches found.\n");
        return md;
    }

    for entry in &result.matches {
        md.push_str(&format!("## {} ({})\n\n", entry.canonical_name, entry.kind));
        md.push_str(&format!("- Anchor: `{}#{}`\n", entry.spec, entry.anchor));
        if let Some(owner) = &entry.owner {
            md.push_str(&format!("- Owner: `{}`\n", owner));
        }
        md.push_str(&format!("- Name: `{}`\n", entry.name));
        if let Some(title) = &entry.title {
            md.push_str(&format!("- Title: {}\n", title));
        }
        if let Some(idl_text) = &entry.idl_text {
            md.push_str("\n```webidl\n");
            md.push_str(idl_text);
            md.push_str("\n```\n");
        }
        md.push('\n');
    }

    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NavEntry, Navigation, RefEntry, RefsMatch};

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
                snippet: "An object A is before an object B in <mark>tree order</mark>..."
                    .to_string(),
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
            query: "HTML#navigate".to_string(),
            direction: "both".to_string(),
            matches: vec![RefsMatch {
                spec: "HTML".to_string(),
                anchor: "navigate".to_string(),
                title: None,
                section_type: "algorithm".to_string(),
                resolution: "exact".to_string(),
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
            }],
        };

        let md = refs(&result);
        assert!(md.contains("# refs: `HTML#navigate`"));
        assert!(md.contains("## HTML#navigate"));
        assert!(md.contains("Outgoing: 2"));
        assert!(md.contains("- URL#concept-url"));
        assert!(md.contains("- INFRA#assert"));
        assert!(md.contains("Incoming: 1"));
        assert!(md.contains("- HTML#navigate-fragid"));
    }

    #[test]
    fn test_refs_format_no_matches() {
        let result = RefsResult {
            query: "HTML#orphan".to_string(),
            direction: "both".to_string(),
            matches: vec![],
        };

        let md = refs(&result);
        assert!(md.contains("# refs: `HTML#orphan`"));
        assert!(md.contains("No matches found"));
    }
}
