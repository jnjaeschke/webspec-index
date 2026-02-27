use serde::{Deserialize, Serialize};

/// Static metadata for a spec
#[derive(Debug, Clone)]
pub struct SpecInfo {
    pub name: &'static str,
    pub base_url: &'static str,
    pub provider: &'static str,
    pub github_repo: &'static str,
}

/// Type of a section
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionType {
    Heading,
    Algorithm,
    Definition,
    Idl,
    Prose,
}

impl SectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SectionType::Heading => "heading",
            SectionType::Algorithm => "algorithm",
            SectionType::Definition => "definition",
            SectionType::Idl => "idl",
            SectionType::Prose => "prose",
        }
    }
}

impl std::str::FromStr for SectionType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "heading" => Ok(SectionType::Heading),
            "algorithm" => Ok(SectionType::Algorithm),
            "definition" => Ok(SectionType::Definition),
            "idl" => Ok(SectionType::Idl),
            "prose" => Ok(SectionType::Prose),
            _ => Err(()),
        }
    }
}

/// A parsed section from the spec HTML
#[derive(Debug, Clone)]
pub struct ParsedSection {
    pub anchor: String,
    pub title: Option<String>,
    pub content_text: Option<String>,
    pub section_type: SectionType,
    pub parent_anchor: Option<String>,
    pub prev_anchor: Option<String>,
    pub next_anchor: Option<String>,
    pub depth: Option<u8>, // 2-6 for headings
}

/// A cross-reference found in the spec
#[derive(Debug, Clone)]
pub struct ParsedReference {
    pub from_anchor: String,
    pub to_spec: String, // Target spec name (same as source for intra-spec refs)
    pub to_anchor: String,
}

/// Complete parsed spec
#[derive(Debug)]
pub struct ParsedSpec {
    pub sections: Vec<ParsedSection>,
    pub references: Vec<ParsedReference>,
}

/// JSON output for query command
#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    pub spec: String,
    pub sha: String,
    pub anchor: String,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub section_type: String,
    pub content: Option<String>,
    pub navigation: Navigation,
    pub outgoing_refs: Vec<RefEntry>,
    pub incoming_refs: Vec<RefEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Navigation {
    pub parent: Option<NavEntry>,
    pub prev: Option<NavEntry>,
    pub next: Option<NavEntry>,
    pub children: Vec<NavEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NavEntry {
    pub anchor: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RefEntry {
    pub spec: String,
    pub anchor: String,
}

/// JSON output for exists command
#[derive(Debug, Serialize)]
pub struct ExistsResult {
    pub exists: bool,
    pub spec: String,
    pub anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub section_type: Option<String>,
}

/// JSON output for anchors command
#[derive(Debug, Serialize)]
pub struct AnchorsResult {
    pub pattern: String,
    pub results: Vec<AnchorEntry>,
}

#[derive(Debug, Serialize)]
pub struct AnchorEntry {
    pub spec: String,
    pub anchor: String,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub section_type: String,
}

/// JSON output for search command
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub query: String,
    pub results: Vec<SearchEntry>,
}

#[derive(Debug, Serialize)]
pub struct SearchEntry {
    pub spec: String,
    pub anchor: String,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub section_type: String,
    pub snippet: String,
}

/// JSON output for list command
#[derive(Debug, Serialize)]
pub struct ListEntry {
    pub anchor: String,
    pub title: Option<String>,
    pub depth: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// JSON output for spec_urls command
#[derive(Debug, Serialize)]
pub struct SpecUrlEntry {
    pub spec: String,
    pub base_url: String,
}

/// JSON output for update command
#[derive(Debug, Serialize)]
pub struct UpdateEntry {
    pub spec: String,
    pub updated: bool,
}

/// JSON output for refs command
#[derive(Debug, Serialize)]
pub struct RefsResult {
    pub anchor: String,
    pub direction: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outgoing: Option<Vec<RefEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming: Option<Vec<RefEntry>>,
}
