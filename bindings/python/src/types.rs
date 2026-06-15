//! Typed result classes mirroring `webspec_index::model` (and the analyze
//! views), exposed to Python as frozen dataclasses-like `#[pyclass]` objects.
//!
//! Each class:
//! - exposes every field as a read-only attribute (`get_all`),
//! - serializes to the same JSON shape the CLI emits (`to_json` / `to_dict`),
//! - has a `__repr__` for friendly REPL output.
//!
//! The structs are owned copies converted from the core model via `From`, so
//! the core crate stays free of any PyO3 dependency.

use pyo3::prelude::*;
use pyo3::types::PyAny;
use serde::Serialize;

use webspec_index::analyze::file::{
    CoverageSummary as CoreCoverage, FileAnalysisView, ScopeAnalysisView, StepAnalysisView,
};
use webspec_index::model;

use crate::WebspecError;

/// Generate the shared `to_json` / `to_dict` / `__repr__` methods for a class.
macro_rules! py_data {
    ($name:ident) => {
        #[pymethods]
        impl $name {
            /// Serialize to a JSON string (identical to the CLI's JSON output).
            fn to_json(&self) -> PyResult<String> {
                serde_json::to_string(self).map_err(|e| WebspecError::new_err(e.to_string()))
            }

            /// Convert to a plain Python ``dict`` (via JSON).
            fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
                let s = self.to_json()?;
                py.import("json")?.call_method1("loads", (s,))
            }

            fn __repr__(&self) -> String {
                format!(
                    "{}({})",
                    stringify!($name),
                    serde_json::to_string(self).unwrap_or_default()
                )
            }
        }
    };
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct NavEntry {
    pub anchor: String,
    pub title: Option<String>,
}
py_data!(NavEntry);

impl From<&model::NavEntry> for NavEntry {
    fn from(e: &model::NavEntry) -> Self {
        NavEntry {
            anchor: e.anchor.clone(),
            title: e.title.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct RefEntry {
    pub spec: String,
    pub anchor: String,
}
py_data!(RefEntry);

impl From<&model::RefEntry> for RefEntry {
    fn from(e: &model::RefEntry) -> Self {
        RefEntry {
            spec: e.spec.clone(),
            anchor: e.anchor.clone(),
        }
    }
}

fn refs(v: &[model::RefEntry]) -> Vec<RefEntry> {
    v.iter().map(RefEntry::from).collect()
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct Navigation {
    pub parent: Option<NavEntry>,
    pub prev: Option<NavEntry>,
    pub next: Option<NavEntry>,
    pub children: Vec<NavEntry>,
}
py_data!(Navigation);

impl From<&model::Navigation> for Navigation {
    fn from(n: &model::Navigation) -> Self {
        Navigation {
            parent: n.parent.as_ref().map(NavEntry::from),
            prev: n.prev.as_ref().map(NavEntry::from),
            next: n.next.as_ref().map(NavEntry::from),
            children: n.children.iter().map(NavEntry::from).collect(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
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
py_data!(QueryResult);

impl From<&model::QueryResult> for QueryResult {
    fn from(r: &model::QueryResult) -> Self {
        QueryResult {
            spec: r.spec.clone(),
            sha: r.sha.clone(),
            anchor: r.anchor.clone(),
            title: r.title.clone(),
            section_type: r.section_type.clone(),
            content: r.content.clone(),
            navigation: Navigation::from(&r.navigation),
            outgoing_refs: refs(&r.outgoing_refs),
            incoming_refs: refs(&r.incoming_refs),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct ExistsResult {
    pub exists: bool,
    pub spec: String,
    pub anchor: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub section_type: Option<String>,
}
py_data!(ExistsResult);

impl From<&model::ExistsResult> for ExistsResult {
    fn from(r: &model::ExistsResult) -> Self {
        ExistsResult {
            exists: r.exists,
            spec: r.spec.clone(),
            anchor: r.anchor.clone(),
            section_type: r.section_type.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct AnchorEntry {
    pub spec: String,
    pub anchor: String,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub section_type: String,
}
py_data!(AnchorEntry);

impl From<&model::AnchorEntry> for AnchorEntry {
    fn from(e: &model::AnchorEntry) -> Self {
        AnchorEntry {
            spec: e.spec.clone(),
            anchor: e.anchor.clone(),
            title: e.title.clone(),
            section_type: e.section_type.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct AnchorsResult {
    pub pattern: String,
    pub results: Vec<AnchorEntry>,
}
py_data!(AnchorsResult);

impl From<&model::AnchorsResult> for AnchorsResult {
    fn from(r: &model::AnchorsResult) -> Self {
        AnchorsResult {
            pattern: r.pattern.clone(),
            results: r.results.iter().map(AnchorEntry::from).collect(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct SearchEntry {
    pub spec: String,
    pub anchor: String,
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub section_type: String,
    pub snippet: String,
}
py_data!(SearchEntry);

impl From<&model::SearchEntry> for SearchEntry {
    fn from(e: &model::SearchEntry) -> Self {
        SearchEntry {
            spec: e.spec.clone(),
            anchor: e.anchor.clone(),
            title: e.title.clone(),
            section_type: e.section_type.clone(),
            snippet: e.snippet.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct SearchResult {
    pub query: String,
    pub results: Vec<SearchEntry>,
}
py_data!(SearchResult);

impl From<&model::SearchResult> for SearchResult {
    fn from(r: &model::SearchResult) -> Self {
        SearchResult {
            query: r.query.clone(),
            results: r.results.iter().map(SearchEntry::from).collect(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct ListEntry {
    pub anchor: String,
    pub title: Option<String>,
    pub depth: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}
py_data!(ListEntry);

impl From<&model::ListEntry> for ListEntry {
    fn from(e: &model::ListEntry) -> Self {
        ListEntry {
            anchor: e.anchor.clone(),
            title: e.title.clone(),
            depth: e.depth,
            parent: e.parent.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct SpecUrlEntry {
    pub spec: String,
    pub base_url: String,
}
py_data!(SpecUrlEntry);

impl From<&model::SpecUrlEntry> for SpecUrlEntry {
    fn from(e: &model::SpecUrlEntry) -> Self {
        SpecUrlEntry {
            spec: e.spec.clone(),
            base_url: e.base_url.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct RefsMatch {
    pub spec: String,
    pub anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub section_type: String,
    pub resolution: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outgoing: Option<Vec<RefEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming: Option<Vec<RefEntry>>,
}
py_data!(RefsMatch);

impl From<&model::RefsMatch> for RefsMatch {
    fn from(m: &model::RefsMatch) -> Self {
        RefsMatch {
            spec: m.spec.clone(),
            anchor: m.anchor.clone(),
            title: m.title.clone(),
            section_type: m.section_type.clone(),
            resolution: m.resolution.clone(),
            outgoing: m.outgoing.as_ref().map(|v| refs(v)),
            incoming: m.incoming.as_ref().map(|v| refs(v)),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct RefsResult {
    pub query: String,
    pub direction: String,
    pub matches: Vec<RefsMatch>,
}
py_data!(RefsResult);

impl From<&model::RefsResult> for RefsResult {
    fn from(r: &model::RefsResult) -> Self {
        RefsResult {
            query: r.query.clone(),
            direction: r.direction.clone(),
            matches: r.matches.iter().map(RefsMatch::from).collect(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct IdlEntry {
    pub spec: String,
    pub anchor: String,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub canonical_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idl_text: Option<String>,
}
py_data!(IdlEntry);

impl From<&model::IdlEntry> for IdlEntry {
    fn from(e: &model::IdlEntry) -> Self {
        IdlEntry {
            spec: e.spec.clone(),
            anchor: e.anchor.clone(),
            kind: e.kind.clone(),
            name: e.name.clone(),
            owner: e.owner.clone(),
            canonical_name: e.canonical_name.clone(),
            title: e.title.clone(),
            idl_text: e.idl_text.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct IdlResult {
    pub query: String,
    pub matches: Vec<IdlEntry>,
}
py_data!(IdlResult);

impl From<&model::IdlResult> for IdlResult {
    fn from(r: &model::IdlResult) -> Self {
        IdlResult {
            query: r.query.clone(),
            matches: r.matches.iter().map(IdlEntry::from).collect(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct GraphRoot {
    pub spec: String,
    pub anchor: String,
}
py_data!(GraphRoot);

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub spec: String,
    pub anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub section_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_role: Option<String>,
}
py_data!(GraphNode);

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}
py_data!(GraphEdge);

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct GraphResult {
    pub root: GraphRoot,
    pub direction: String,
    pub max_depth: usize,
    pub max_nodes: usize,
    pub truncated: bool,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}
py_data!(GraphResult);

impl From<&model::GraphResult> for GraphResult {
    fn from(g: &model::GraphResult) -> Self {
        GraphResult {
            root: GraphRoot {
                spec: g.root.spec.clone(),
                anchor: g.root.anchor.clone(),
            },
            direction: g.direction.clone(),
            max_depth: g.max_depth,
            max_nodes: g.max_nodes,
            truncated: g.truncated,
            nodes: g
                .nodes
                .iter()
                .map(|n| GraphNode {
                    id: n.id.clone(),
                    spec: n.spec.clone(),
                    anchor: n.anchor.clone(),
                    title: n.title.clone(),
                    section_type: n.section_type.clone(),
                    filter_role: n.filter_role.clone(),
                })
                .collect(),
            edges: g
                .edges
                .iter()
                .map(|e| GraphEdge {
                    from: e.from.clone(),
                    to: e.to.clone(),
                    kind: e.kind.clone(),
                })
                .collect(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct PrDiffSummary {
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
}
py_data!(PrDiffSummary);

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct PrDiffEntry {
    pub anchor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub change_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_content: Option<String>,
}
py_data!(PrDiffEntry);

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct PrDiffResult {
    pub spec: String,
    pub pr_number: i64,
    pub head_sha: String,
    pub merge_base_sha: String,
    pub summary: PrDiffSummary,
    pub changes: Vec<PrDiffEntry>,
}
py_data!(PrDiffResult);

impl From<&model::PrDiffResult> for PrDiffResult {
    fn from(d: &model::PrDiffResult) -> Self {
        PrDiffResult {
            spec: d.spec.clone(),
            pr_number: d.pr_number,
            head_sha: d.head_sha.clone(),
            merge_base_sha: d.merge_base_sha.clone(),
            summary: PrDiffSummary {
                added: d.summary.added,
                removed: d.summary.removed,
                modified: d.summary.modified,
            },
            changes: d
                .changes
                .iter()
                .map(|c| PrDiffEntry {
                    anchor: c.anchor.clone(),
                    title: c.title.clone(),
                    change_type: c.change_type.clone(),
                    old_content: c.old_content.clone(),
                    new_content: c.new_content.clone(),
                })
                .collect(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct UpdateEntry {
    pub spec: String,
    pub updated: bool,
}
py_data!(UpdateEntry);

// ── Analyze view types ───────────────────────────────────────────────

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct CoverageSummary {
    pub total: usize,
    pub implemented: usize,
    pub missing: Vec<Vec<u32>>,
    pub warnings: usize,
    pub reordered: usize,
}
py_data!(CoverageSummary);

impl From<&CoreCoverage> for CoverageSummary {
    fn from(c: &CoreCoverage) -> Self {
        CoverageSummary {
            total: c.total,
            implemented: c.implemented,
            missing: c.missing.clone(),
            warnings: c.warnings,
            reordered: c.reordered,
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct StepValidation {
    pub line: usize,
    pub col: usize,
    pub step: Vec<u32>,
    pub comment_text: String,
    pub result: String,
    pub spec_text: String,
}
py_data!(StepValidation);

impl From<&StepAnalysisView> for StepValidation {
    fn from(s: &StepAnalysisView) -> Self {
        StepValidation {
            line: s.line,
            col: s.col,
            step: s.step.clone(),
            comment_text: s.comment_text.clone(),
            result: s.result.clone(),
            spec_text: s.spec_text.clone(),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct ScopeAnalysis {
    pub spec: String,
    pub anchor: String,
    pub url: String,
    pub line: usize,
    pub col: usize,
    pub validations: Vec<StepValidation>,
    pub coverage: Option<CoverageSummary>,
}
py_data!(ScopeAnalysis);

impl From<&ScopeAnalysisView> for ScopeAnalysis {
    fn from(s: &ScopeAnalysisView) -> Self {
        ScopeAnalysis {
            spec: s.spec.clone(),
            anchor: s.anchor.clone(),
            url: s.url.clone(),
            line: s.line,
            col: s.col,
            validations: s.validations.iter().map(StepValidation::from).collect(),
            coverage: s.coverage.as_ref().map(CoverageSummary::from),
        }
    }
}

#[pyclass(frozen, get_all, skip_from_py_object)]
#[derive(Clone, Serialize)]
pub struct FileAnalysis {
    pub file: String,
    pub scopes: Vec<ScopeAnalysis>,
}
py_data!(FileAnalysis);

impl FileAnalysis {
    pub fn new(file: String, view: &FileAnalysisView) -> Self {
        FileAnalysis {
            file,
            scopes: view.scopes.iter().map(ScopeAnalysis::from).collect(),
        }
    }
}

/// Register all type classes on the module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NavEntry>()?;
    m.add_class::<RefEntry>()?;
    m.add_class::<Navigation>()?;
    m.add_class::<QueryResult>()?;
    m.add_class::<ExistsResult>()?;
    m.add_class::<AnchorEntry>()?;
    m.add_class::<AnchorsResult>()?;
    m.add_class::<SearchEntry>()?;
    m.add_class::<SearchResult>()?;
    m.add_class::<ListEntry>()?;
    m.add_class::<SpecUrlEntry>()?;
    m.add_class::<RefsMatch>()?;
    m.add_class::<RefsResult>()?;
    m.add_class::<IdlEntry>()?;
    m.add_class::<IdlResult>()?;
    m.add_class::<GraphRoot>()?;
    m.add_class::<GraphNode>()?;
    m.add_class::<GraphEdge>()?;
    m.add_class::<GraphResult>()?;
    m.add_class::<PrDiffSummary>()?;
    m.add_class::<PrDiffEntry>()?;
    m.add_class::<PrDiffResult>()?;
    m.add_class::<UpdateEntry>()?;
    m.add_class::<CoverageSummary>()?;
    m.add_class::<StepValidation>()?;
    m.add_class::<ScopeAnalysis>()?;
    m.add_class::<FileAnalysis>()?;
    Ok(())
}
