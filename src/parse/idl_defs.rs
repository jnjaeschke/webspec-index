use crate::model::ParsedIdlDefinition;
use scraper::Html;

fn normalize_owner(raw_owner: &str) -> String {
    raw_owner
        .split('/')
        .next()
        .unwrap_or(raw_owner)
        .trim()
        .to_string()
}

fn base_name(kind: &str, raw_name: &str) -> String {
    if kind == "constructor" {
        return "constructor".to_string();
    }

    if kind == "method" {
        let trimmed = raw_name.trim();
        return trimmed
            .split('(')
            .next()
            .unwrap_or(trimmed)
            .trim()
            .to_string();
    }

    raw_name.trim().to_string()
}

fn canonical_name(owner: Option<&str>, kind: &str, raw_name: &str) -> String {
    let base = base_name(kind, raw_name);
    if let Some(owner) = owner {
        let owner = normalize_owner(owner);
        if !owner.is_empty() && !base.is_empty() {
            return format!("{owner}.{base}");
        }
    }
    base
}

fn nearest_pre_idl_text(element: &scraper::ElementRef) -> Option<String> {
    let mut current = element.parent();
    while let Some(node) = current {
        if let Some(parent) = scraper::ElementRef::wrap(node) {
            if parent.value().name() == "pre" {
                let text = super::idl::extract_idl_text(&parent);
                if !text.trim().is_empty() {
                    return Some(text);
                }
                return None;
            }
        }
        current = node.parent();
    }
    None
}

/// Extract all WebIDL definitions from dfn nodes carrying `data-dfn-type`.
pub fn extract_idl_definitions(html: &str) -> Vec<ParsedIdlDefinition> {
    let document = Html::parse_document(html);
    let selector = scraper::Selector::parse("dfn[id][data-dfn-type]").expect("valid selector");
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    for dfn in document.select(&selector) {
        let kind = dfn
            .value()
            .attr("data-dfn-type")
            .unwrap_or_default()
            .trim()
            .to_string();
        if kind.is_empty() || kind == "argument" {
            continue;
        }

        let anchor = match dfn.value().attr("id") {
            Some(id) if !id.trim().is_empty() => id.trim().to_string(),
            _ => continue,
        };

        let raw_name = dfn.text().collect::<String>().trim().to_string();
        if raw_name.is_empty() {
            continue;
        }

        let owner = dfn
            .value()
            .attr("data-dfn-for")
            .map(normalize_owner)
            .filter(|o| !o.is_empty());
        let canonical_name = canonical_name(owner.as_deref(), &kind, &raw_name);
        if canonical_name.is_empty() {
            continue;
        }

        // Deduplicate by anchor/kind within one parsed document.
        if !seen.insert((anchor.clone(), kind.clone())) {
            continue;
        }

        out.push(ParsedIdlDefinition {
            anchor,
            name: raw_name,
            owner,
            kind,
            canonical_name,
            idl_text: nearest_pre_idl_text(&dfn),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_interface_and_member_defs() {
        let html = r#"
            <pre class="idl">
                interface <dfn id="dom-window" data-dfn-type="interface"><code>Window</code></dfn> {
                  attribute <dfn id="dom-window-navigation" data-dfn-type="attribute" data-dfn-for="Window"><code>navigation</code></dfn>;
                  undefined <dfn id="dom-window-open" data-dfn-type="method" data-dfn-for="Window"><code>open(url)</code></dfn>;
                };
            </pre>
        "#;

        let defs = extract_idl_definitions(html);
        assert!(defs.iter().any(|d| d.canonical_name == "Window"));
        assert!(defs.iter().any(|d| d.canonical_name == "Window.navigation"));
        assert!(defs.iter().any(|d| d.canonical_name == "Window.open"));
        assert!(defs.iter().all(|d| d.idl_text.is_some()));
    }

    #[test]
    fn skips_argument_defs() {
        let html = r#"
            <pre class="idl">
                <dfn data-dfn-for="Window/open(url)" data-dfn-type="argument" id="dom-window-open-url"><code>url</code></dfn>
            </pre>
        "#;
        let defs = extract_idl_definitions(html);
        assert!(defs.is_empty());
    }
}
