//! PyO3 bindings for webspec-index
//!
//! This module exposes the Rust library to Python via FFI.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::sync::OnceLock;

/// Global tokio runtime for async operations
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Get or create a tokio runtime for async operations
fn get_runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("Failed to create tokio runtime"))
}

/// Convert Rust Result to Python result (JSON string on success)
fn to_py_result<T: serde::Serialize>(result: anyhow::Result<T>) -> PyResult<String> {
    match result {
        Ok(value) => Ok(serde_json::to_string(&value)
            .map_err(|e| PyRuntimeError::new_err(format!("JSON serialization error: {}", e)))?),
        Err(e) => Err(PyRuntimeError::new_err(e.to_string())),
    }
}

/// Query a specific section in a specification
///
/// Args:
///     spec_anchor (str): Spec and anchor in format "SPEC#anchor" (e.g., "HTML#navigate")
///     sha (str | None): Optional commit SHA for specific version
///
/// Returns:
///     str: JSON string with section details, navigation, and references
#[pyfunction]
#[pyo3(signature = (spec_anchor, sha=None))]
fn query(spec_anchor: String, sha: Option<String>) -> PyResult<String> {
    let rt = get_runtime();
    let result = rt.block_on(crate::query_section(&spec_anchor, sha.as_deref()));
    to_py_result(result)
}

/// Full-text search across specifications
///
/// Args:
///     query (str): Text to search for
///     spec (str | None): Optional spec name to limit search
///     limit (int): Maximum number of results (default 20)
///
/// Returns:
///     str: JSON string with matching sections and snippets
#[pyfunction]
#[pyo3(signature = (query, spec=None, limit=20))]
fn search(query: String, spec: Option<String>, limit: usize) -> PyResult<String> {
    let result = crate::search_sections(&query, spec.as_deref(), limit);
    to_py_result(result)
}

/// Check if a section exists
///
/// Args:
///     spec_anchor (str): Spec and anchor in format "SPEC#anchor"
///
/// Returns:
///     str: JSON string with existence status and section type
#[pyfunction]
fn exists(spec_anchor: String) -> PyResult<String> {
    let rt = get_runtime();
    let result = rt.block_on(crate::check_exists(&spec_anchor));
    to_py_result(result)
}

/// Find anchors matching a glob pattern
///
/// Args:
///     pattern (str): Glob pattern (e.g., "*-tree", "concept-*")
///     spec (str | None): Optional spec name to limit search
///     limit (int): Maximum number of results (default 50)
///
/// Returns:
///     str: JSON string with matching anchors
#[pyfunction]
#[pyo3(signature = (pattern, spec=None, limit=50))]
fn anchors(pattern: String, spec: Option<String>, limit: usize) -> PyResult<String> {
    let result = crate::find_anchors(&pattern, spec.as_deref(), limit);
    to_py_result(result)
}

/// List all headings in a specification
///
/// Args:
///     spec (str): Spec name (e.g., "HTML", "DOM")
///     sha (str | None): Optional commit SHA for specific version
///
/// Returns:
///     str: JSON string with list of headings
#[pyfunction]
#[pyo3(signature = (spec, sha=None))]
fn list_headings(spec: String, sha: Option<String>) -> PyResult<String> {
    let rt = get_runtime();
    let result = rt.block_on(crate::list_headings(&spec, sha.as_deref()));
    to_py_result(result)
}

/// Get cross-references for a section
///
/// Args:
///     spec_anchor (str): Spec and anchor in format "SPEC#anchor"
///     direction (str): "incoming", "outgoing", or "both" (default: "both")
///     sha (str | None): Optional commit SHA for specific version
///
/// Returns:
///     str: JSON string with incoming and/or outgoing references
#[pyfunction]
#[pyo3(signature = (spec_anchor, direction="both".to_string(), sha=None))]
fn refs(spec_anchor: String, direction: String, sha: Option<String>) -> PyResult<String> {
    let rt = get_runtime();
    let result = rt.block_on(crate::get_references(
        &spec_anchor,
        &direction,
        sha.as_deref(),
    ));
    to_py_result(result)
}

/// Update specifications to latest versions
///
/// Args:
///     spec (str | None): Optional spec name (updates all if None)
///     force (bool): Force update even if recently checked (default: False)
///
/// Returns:
///     str: JSON string with list of updated specs
#[pyfunction]
#[pyo3(signature = (spec=None, force=false))]
fn update(spec: Option<String>, force: bool) -> PyResult<String> {
    let rt = get_runtime();
    let result = rt.block_on(crate::update_specs(spec.as_deref(), force));
    to_py_result(result)
}

/// Clear the database (remove all indexed data)
///
/// Returns:
///     str: Path to the deleted database file
#[pyfunction]
fn clear_db() -> PyResult<String> {
    crate::clear_database().map_err(|e| PyRuntimeError::new_err(e.to_string()))
}

/// WebSpec-Index Python module
#[pymodule]
fn _webspec_index(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(query, m)?)?;
    m.add_function(wrap_pyfunction!(search, m)?)?;
    m.add_function(wrap_pyfunction!(exists, m)?)?;
    m.add_function(wrap_pyfunction!(anchors, m)?)?;
    m.add_function(wrap_pyfunction!(list_headings, m)?)?;
    m.add_function(wrap_pyfunction!(refs, m)?)?;
    m.add_function(wrap_pyfunction!(update, m)?)?;
    m.add_function(wrap_pyfunction!(clear_db, m)?)?;
    Ok(())
}
