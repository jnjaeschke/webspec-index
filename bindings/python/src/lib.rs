//! Python bindings for `webspec-index`.
//!
//! Exposes the query/search/graph/idl/analyze API of the core crate as a
//! synchronous Python module. Each call drives the core's async functions on a
//! shared multi-threaded Tokio runtime and returns typed result objects (see
//! [`types`]).

mod types;

use std::future::Future;
use std::path::Path;
use std::sync::OnceLock;

use pyo3::prelude::*;
use tokio::runtime::Runtime;

use webspec_index::analyze::file::FileAnalysisView;
use webspec_index::analyze::orchestrate;
use webspec_index::model;

use types::*;

pyo3::create_exception!(
    _webspec_index,
    WebspecError,
    pyo3::exceptions::PyException,
    "Raised when a webspec-index operation fails."
);

/// Shared multi-threaded Tokio runtime.
///
/// Multi-threaded is required: spec resolution during `analyze` uses
/// `block_in_place`, which only works on a multi-threaded runtime.
fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build Tokio runtime")
    })
}

fn to_py_err(e: anyhow::Error) -> PyErr {
    WebspecError::new_err(format!("{e:#}"))
}

/// Drive an async core call to completion, mapping errors to `WebspecError`.
fn run<F, T>(fut: F) -> PyResult<T>
where
    F: Future<Output = anyhow::Result<T>>,
{
    runtime().block_on(fut).map_err(to_py_err)
}

fn pr_opts(pr: Option<i64>, force_update: bool) -> Option<model::PrOpts> {
    pr.map(|pr_number| model::PrOpts {
        pr_number,
        force_update,
    })
}

/// Query a specific section: ``SPEC#anchor`` or a full spec URL.
#[pyfunction]
#[pyo3(signature = (spec_anchor, pr=None, force_update=false))]
fn query(spec_anchor: &str, pr: Option<i64>, force_update: bool) -> PyResult<QueryResult> {
    let opts = pr_opts(pr, force_update);
    let r = run(webspec_index::query_section(spec_anchor, opts.as_ref()))?;
    Ok((&r).into())
}

/// Check whether a section exists.
#[pyfunction]
#[pyo3(signature = (spec_anchor, pr=None, force_update=false))]
fn exists(spec_anchor: &str, pr: Option<i64>, force_update: bool) -> PyResult<ExistsResult> {
    let opts = pr_opts(pr, force_update);
    let r = run(webspec_index::check_exists(spec_anchor, opts.as_ref()))?;
    Ok((&r).into())
}

/// Full-text search across indexed specifications.
#[pyfunction]
#[pyo3(signature = (query, spec=None, limit=20, pr=None, force_update=false))]
fn search(
    query: &str,
    spec: Option<&str>,
    limit: u32,
    pr: Option<i64>,
    force_update: bool,
) -> PyResult<SearchResult> {
    let opts = pr_opts(pr, force_update);
    let r = run(webspec_index::search_sections(
        query,
        spec,
        limit,
        opts.as_ref(),
    ))?;
    Ok((&r).into())
}

/// Find anchors matching a glob pattern (``*`` wildcard).
#[pyfunction]
#[pyo3(signature = (pattern, spec=None, limit=50, pr=None, force_update=false))]
fn anchors(
    pattern: &str,
    spec: Option<&str>,
    limit: u32,
    pr: Option<i64>,
    force_update: bool,
) -> PyResult<AnchorsResult> {
    let opts = pr_opts(pr, force_update);
    let r = run(webspec_index::find_anchors(
        pattern,
        spec,
        limit,
        opts.as_ref(),
    ))?;
    Ok((&r).into())
}

/// List all headings in a specification.
#[pyfunction]
#[pyo3(signature = (spec, pr=None, force_update=false))]
fn list_headings(spec: &str, pr: Option<i64>, force_update: bool) -> PyResult<Vec<ListEntry>> {
    let opts = pr_opts(pr, force_update);
    let r = run(webspec_index::list_headings(spec, opts.as_ref()))?;
    Ok(r.iter().map(ListEntry::from).collect())
}

/// Get cross-references for ``SPEC#anchor``, a URL, or an ``Interface.member`` shorthand.
#[pyfunction]
#[pyo3(signature = (target, direction="both", limit=10, pr=None, force_update=false))]
fn refs(
    target: &str,
    direction: &str,
    limit: u32,
    pr: Option<i64>,
    force_update: bool,
) -> PyResult<RefsResult> {
    let opts = pr_opts(pr, force_update);
    let r = run(webspec_index::find_references(
        target,
        direction,
        limit,
        opts.as_ref(),
    ))?;
    Ok((&r).into())
}

/// Build a cross-reference graph rooted at a section.
#[pyfunction]
#[pyo3(signature = (
    spec_anchor,
    direction="outgoing",
    max_depth=2,
    max_nodes=150,
    include=None,
    exclude=None,
    same_spec_only=false,
))]
#[allow(clippy::too_many_arguments)]
fn graph(
    spec_anchor: &str,
    direction: &str,
    max_depth: usize,
    max_nodes: usize,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    same_spec_only: bool,
) -> PyResult<GraphResult> {
    let include = include.unwrap_or_default();
    let exclude = exclude.unwrap_or_default();
    let r = run(webspec_index::graph_section(
        spec_anchor,
        direction,
        max_depth,
        max_nodes,
        &include,
        &exclude,
        same_spec_only,
    ))?;
    Ok((&r).into())
}

/// Query dedicated WebIDL definitions.
#[pyfunction]
#[pyo3(signature = (query, spec=None, limit=20, pr=None, force_update=false))]
fn idl(
    query: &str,
    spec: Option<&str>,
    limit: u32,
    pr: Option<i64>,
    force_update: bool,
) -> PyResult<IdlResult> {
    let opts = pr_opts(pr, force_update);
    let r = run(webspec_index::query_idl(query, spec, limit, opts.as_ref()))?;
    Ok((&r).into())
}

/// Diff a WHATWG PR preview against its merge base for ``spec``.
#[pyfunction]
#[pyo3(signature = (spec, pr, force_update=false))]
fn pr_diff(spec: &str, pr: i64, force_update: bool) -> PyResult<PrDiffResult> {
    let opts = model::PrOpts {
        pr_number: pr,
        force_update,
    };
    let r = run(webspec_index::pr_diff(spec, &opts))?;
    Ok((&r).into())
}

/// Update specs to latest versions (all indexed specs if ``spec`` is None).
#[pyfunction]
#[pyo3(signature = (spec=None, force=false))]
fn update(spec: Option<&str>, force: bool) -> PyResult<Vec<UpdateEntry>> {
    let r = run(webspec_index::update_specs(spec, force))?;
    Ok(r.into_iter()
        .map(|(spec, snapshot_id)| UpdateEntry {
            spec,
            updated: snapshot_id.is_some(),
        })
        .collect())
}

/// Clear cached PR preview data, or list cached PRs when called with no args.
#[pyfunction]
#[pyo3(signature = (spec=None, pr=None, all=false))]
fn clear_pr<'py>(
    py: Python<'py>,
    spec: Option<&str>,
    pr: Option<i64>,
    all: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let value = webspec_index::clear_pr_data(spec, pr, all).map_err(to_py_err)?;
    let s = serde_json::to_string(&value).map_err(|e| WebspecError::new_err(e.to_string()))?;
    py.import("json")?.call_method1("loads", (s,))
}

/// Delete the local database. Returns the path that was removed.
#[pyfunction]
fn clear_db() -> PyResult<String> {
    webspec_index::clear_database().map_err(to_py_err)
}

/// List indexed/discovered spec names and base URLs.
#[pyfunction]
fn specs() -> Vec<SpecUrlEntry> {
    webspec_index::spec_urls()
        .iter()
        .map(SpecUrlEntry::from)
        .collect()
}

/// Parse ``SPEC#anchor`` or a spec URL into ``(spec, anchor, base_url)``.
#[pyfunction]
fn parse_anchor(input: &str) -> PyResult<(String, String, Option<String>)> {
    webspec_index::parse_spec_anchor(input).map_err(to_py_err)
}

/// Analyze a source file or directory for spec references and step-comment validation.
#[pyfunction]
#[pyo3(signature = (path, recursive=false, threshold=0.85))]
fn analyze(path: &str, recursive: bool, threshold: f64) -> PyResult<Vec<FileAnalysis>> {
    let run_result = run(orchestrate::analyze_paths(
        Path::new(path),
        recursive,
        threshold,
    ))?;
    Ok(run_result
        .files
        .iter()
        .map(|f| {
            let view = FileAnalysisView::from(&f.analysis);
            FileAnalysis::new(f.path.to_string_lossy().into_owned(), &view)
        })
        .collect())
}

#[pymodule]
fn _webspec_index(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("WebspecError", m.py().get_type::<WebspecError>())?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;

    types::register(m)?;

    m.add_function(wrap_pyfunction!(query, m)?)?;
    m.add_function(wrap_pyfunction!(exists, m)?)?;
    m.add_function(wrap_pyfunction!(search, m)?)?;
    m.add_function(wrap_pyfunction!(anchors, m)?)?;
    m.add_function(wrap_pyfunction!(list_headings, m)?)?;
    m.add_function(wrap_pyfunction!(refs, m)?)?;
    m.add_function(wrap_pyfunction!(graph, m)?)?;
    m.add_function(wrap_pyfunction!(idl, m)?)?;
    m.add_function(wrap_pyfunction!(pr_diff, m)?)?;
    m.add_function(wrap_pyfunction!(update, m)?)?;
    m.add_function(wrap_pyfunction!(clear_pr, m)?)?;
    m.add_function(wrap_pyfunction!(clear_db, m)?)?;
    m.add_function(wrap_pyfunction!(specs, m)?)?;
    m.add_function(wrap_pyfunction!(parse_anchor, m)?)?;
    m.add_function(wrap_pyfunction!(analyze, m)?)?;

    Ok(())
}
