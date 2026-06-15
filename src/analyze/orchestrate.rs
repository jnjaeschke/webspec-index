//! Filesystem orchestration for the `analyze` workflow.
//!
//! Collects source files, resolves spec sections from the local DB, and runs
//! per-file analysis. Returns structured results; callers (the CLI, the Python
//! bindings) decide how to render or persist them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::file::{analyze_file, FileAnalysis, SpecResolver};
use super::scanner::SpecUrl;

/// Source file extensions to scan when analyzing directories.
pub const SOURCE_EXTENSIONS: &[&str] = &[
    "cpp", "cc", "cxx", "c", "h", "hpp", "hxx", "rs", "js", "mjs", "jsm", "py", "java",
];

/// Whether `path` has a recognized source-file extension.
pub fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

/// Collect source files to analyze from a file or directory.
pub fn collect_files(path: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        anyhow::bail!("{} is not a file or directory", path.display());
    }
    let mut files = Vec::new();
    let mut dirs = vec![path.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            if ft.is_file() && is_source_file(&entry.path()) {
                files.push(entry.path());
            } else if ft.is_dir() && recursive {
                dirs.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// DB-backed spec resolver for the analyze workflow.
///
/// Uses `DashMap` for thread-safe caching (safe for future parallelization).
pub struct DbResolver {
    cache: dashmap::DashMap<String, Option<String>>,
}

impl DbResolver {
    pub fn new() -> Self {
        DbResolver {
            cache: dashmap::DashMap::new(),
        }
    }

    /// Return all successfully resolved sections as a map of
    /// "SPEC_<spec>_<anchor>" -> content (the same symbol names used in
    /// searchfox analysis records).
    pub fn resolved_sections(&self) -> HashMap<String, String> {
        self.cache
            .iter()
            .filter_map(|entry| {
                let content = entry.value().as_ref()?;
                let (spec, anchor) = entry.key().split_once('#')?;
                let sym = format!("SPEC_{spec}_{anchor}");
                Some((sym, content.clone()))
            })
            .collect()
    }
}

impl Default for DbResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl SpecResolver for DbResolver {
    fn resolve(&self, spec: &str, anchor: &str) -> Option<String> {
        let key = format!("{spec}#{anchor}");
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(crate::query_section(&key, None))
                .ok()
        });
        let content = result.and_then(|r| r.content).filter(|c| !c.is_empty());
        self.cache.insert(key, content.clone());
        content
    }
}

/// A single analyzed file with its path and analysis result.
pub struct AnalyzedFile {
    pub path: PathBuf,
    pub analysis: FileAnalysis,
}

/// Result of analyzing a path (file or directory).
pub struct AnalysisRun {
    /// Total number of source files scanned (before scope filtering).
    pub total_files_scanned: usize,
    /// Files that contained at least one spec scope.
    pub files: Vec<AnalyzedFile>,
    /// Files that could not be read, as (path, error message).
    pub read_errors: Vec<(PathBuf, String)>,
    /// Spec sections resolved during analysis (symbol -> content).
    pub resolved_sections: HashMap<String, String>,
}

/// Analyze a file or directory for spec references and step-comment validation.
///
/// Scans each source file for spec URLs and validates step comments against the
/// referenced spec algorithms (fetched/cached via the local DB). Only files with
/// at least one spec scope are included in [`AnalysisRun::files`].
///
/// Must be called from within a multi-threaded Tokio runtime: spec resolution
/// blocks the current worker thread via `block_in_place`.
pub async fn analyze_paths(path: &Path, recursive: bool, threshold: f64) -> Result<AnalysisRun> {
    let files = collect_files(path, recursive)?;
    let total_files_scanned = files.len();

    let spec_urls: Vec<SpecUrl> = crate::spec_urls()
        .into_iter()
        .map(|e| SpecUrl {
            spec: e.spec,
            base_url: e.base_url,
        })
        .collect();

    let resolver = DbResolver::new();
    let mut analyzed = Vec::new();
    let mut read_errors = Vec::new();

    for file_path in files {
        let text = match std::fs::read_to_string(&file_path) {
            Ok(t) => t,
            Err(e) => {
                read_errors.push((file_path, e.to_string()));
                continue;
            }
        };

        let analysis = analyze_file(&text, &spec_urls, &resolver, threshold);
        if analysis.scopes.is_empty() {
            continue;
        }
        analyzed.push(AnalyzedFile {
            path: file_path,
            analysis,
        });
    }

    Ok(AnalysisRun {
        total_files_scanned,
        resolved_sections: resolver.resolved_sections(),
        files: analyzed,
        read_errors,
    })
}
