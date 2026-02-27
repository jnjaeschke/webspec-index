//! tower-lsp based Language Server implementation.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use regex::Regex;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use super::coverage::{compute_coverage, CoverageResult, StepValidation};
use super::hover::build_hover_content;
use super::matcher::{classify_match, MatchResult};
use super::scanner::{
    build_scopes, build_spec_lookup, build_url_pattern, find_url_at_position, scan_document,
    scan_steps, SpecUrl, StepComment, UrlMatch,
};
use super::steps::{find_step, parse_steps, AlgorithmStep};

use crate::model::QueryResult;

const DEBOUNCE_DELAY_MS: u64 = 300;

/// Versioned cache entry.
#[derive(Clone)]
struct Versioned<T: Clone> {
    version: i32,
    data: T,
}

/// Internal validation result for a single step.
#[derive(Clone)]
struct InternalValidation {
    step: StepComment,
    result: MatchResult,
    spec_text: String,
    algo_name: String,
}

/// Shared state that can be cloned into spawned tasks via Arc.
struct State {
    client: Client,
    fuzzy_threshold: Mutex<f64>,
    url_pattern: Mutex<Option<Regex>>,
    spec_lookup: Mutex<HashMap<String, String>>,
    doc_urls: DashMap<String, Versioned<Vec<UrlMatch>>>,
    query_cache: DashMap<String, QueryResult>,
    algo_steps_cache: DashMap<String, Vec<AlgorithmStep>>,
    doc_validations: DashMap<String, Versioned<Vec<InternalValidation>>>,
    #[allow(clippy::type_complexity)]
    doc_scopes: DashMap<String, Versioned<Vec<(UrlMatch, Vec<StepComment>)>>>,
    doc_coverages: DashMap<String, Versioned<Vec<(UrlMatch, CoverageResult)>>>,
    debounce_tokens: DashMap<String, tokio::sync::watch::Sender<()>>,
    documents: DashMap<String, (i32, String)>,
}

impl State {
    fn new(client: Client) -> Self {
        Self {
            client,
            fuzzy_threshold: Mutex::new(0.85),
            url_pattern: Mutex::new(None),
            spec_lookup: Mutex::new(HashMap::new()),
            doc_urls: DashMap::new(),
            query_cache: DashMap::new(),
            algo_steps_cache: DashMap::new(),
            doc_validations: DashMap::new(),
            doc_scopes: DashMap::new(),
            doc_coverages: DashMap::new(),
            debounce_tokens: DashMap::new(),
            documents: DashMap::new(),
        }
    }

    async fn ensure_pattern(&self) {
        let mut pattern = self.url_pattern.lock().await;
        if pattern.is_none() {
            let spec_entries = crate::spec_urls();
            let spec_urls: Vec<SpecUrl> = spec_entries
                .iter()
                .map(|e| SpecUrl {
                    spec: e.spec.clone(),
                    base_url: e.base_url.clone(),
                })
                .collect();
            *pattern = Some(build_url_pattern(&spec_urls));
            let mut lookup = self.spec_lookup.lock().await;
            *lookup = build_spec_lookup(&spec_urls);
        }
    }

    async fn scan_doc(&self, uri: &str, text: &str, version: i32) -> Vec<UrlMatch> {
        self.ensure_pattern().await;
        if let Some(cached) = self.doc_urls.get(uri) {
            if cached.version == version {
                return cached.data.clone();
            }
        }
        let pattern = self.url_pattern.lock().await;
        let lookup = self.spec_lookup.lock().await;
        let matches = scan_document(text, pattern.as_ref().unwrap(), &lookup);
        self.doc_urls.insert(
            uri.to_string(),
            Versioned {
                version,
                data: matches.clone(),
            },
        );
        matches
    }

    fn query_spec_cached(&self, spec: &str, anchor: &str) -> Option<QueryResult> {
        let key = format!("{spec}#{anchor}");
        if let Some(cached) = self.query_cache.get(&key) {
            return Some(cached.clone());
        }
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(crate::query_section(&key))
                .ok()
        })?;
        self.query_cache.insert(key, result.clone());
        Some(result)
    }

    fn get_algo_steps_cached(&self, anchor: &str, content: &str) -> Option<Vec<AlgorithmStep>> {
        if let Some(cached) = self.algo_steps_cache.get(anchor) {
            return Some(cached.clone());
        }
        let steps = parse_steps(content);
        if steps.is_empty() {
            return None;
        }
        self.algo_steps_cache
            .insert(anchor.to_string(), steps.clone());
        Some(steps)
    }

    async fn validate_doc(&self, uri: &str, text: &str, version: i32) -> Vec<InternalValidation> {
        if let Some(cached) = self.doc_validations.get(uri) {
            if cached.version == version {
                return cached.data.clone();
            }
        }

        let url_matches = self.scan_doc(uri, text, version).await;
        let step_comments = scan_steps(text);

        if url_matches.is_empty() || step_comments.is_empty() {
            self.doc_validations.insert(
                uri.to_string(),
                Versioned {
                    version,
                    data: vec![],
                },
            );
            self.doc_scopes.insert(
                uri.to_string(),
                Versioned {
                    version,
                    data: vec![],
                },
            );
            return vec![];
        }

        let scopes = build_scopes(&url_matches, &step_comments);
        self.doc_scopes.insert(
            uri.to_string(),
            Versioned {
                version,
                data: scopes.clone(),
            },
        );

        let threshold = *self.fuzzy_threshold.lock().await;
        let mut validations = Vec::new();

        for (url_match, steps_in_scope) in &scopes {
            if steps_in_scope.is_empty() {
                continue;
            }
            let result = match self.query_spec_cached(&url_match.spec, &url_match.anchor) {
                Some(r) => r,
                None => continue,
            };
            let content = match &result.content {
                Some(c) if !c.is_empty() => c.clone(),
                _ => continue,
            };
            let algo_steps = match self.get_algo_steps_cached(&url_match.anchor, &content) {
                Some(s) => s,
                None => continue,
            };

            for sc in steps_in_scope {
                let spec_step = find_step(&algo_steps, &sc.number);
                let (match_result, spec_text) = if let Some(ss) = spec_step {
                    (
                        classify_match(&sc.text, &ss.text, threshold),
                        ss.text.clone(),
                    )
                } else {
                    (MatchResult::NotFound, String::new())
                };
                validations.push(InternalValidation {
                    step: sc.clone(),
                    result: match_result,
                    spec_text,
                    algo_name: url_match.anchor.clone(),
                });
            }
        }

        self.doc_validations.insert(
            uri.to_string(),
            Versioned {
                version,
                data: validations.clone(),
            },
        );
        validations
    }

    async fn coverage_doc(
        &self,
        uri: &str,
        text: &str,
        version: i32,
    ) -> Vec<(UrlMatch, CoverageResult)> {
        if let Some(cached) = self.doc_coverages.get(uri) {
            if cached.version == version {
                return cached.data.clone();
            }
        }

        let validations = self.validate_doc(uri, text, version).await;
        if validations.is_empty() {
            self.doc_coverages.insert(
                uri.to_string(),
                Versioned {
                    version,
                    data: vec![],
                },
            );
            return vec![];
        }

        let scopes = match self.doc_scopes.get(uri) {
            Some(s) if s.version == version => s.data.clone(),
            _ => {
                self.doc_coverages.insert(
                    uri.to_string(),
                    Versioned {
                        version,
                        data: vec![],
                    },
                );
                return vec![];
            }
        };

        let mut results = Vec::new();
        for (url_match, steps_in_scope) in &scopes {
            if steps_in_scope.is_empty() {
                continue;
            }
            let algo_steps = match self.algo_steps_cache.get(&url_match.anchor) {
                Some(s) => s.clone(),
                None => continue,
            };
            let scope_lines: std::collections::HashSet<usize> =
                steps_in_scope.iter().map(|s| s.line).collect();
            let scope_vals: Vec<StepValidation> = validations
                .iter()
                .filter(|v| scope_lines.contains(&v.step.line))
                .map(|v| StepValidation {
                    step: v.step.clone(),
                    result: v.result,
                })
                .collect();
            let cov = compute_coverage(&scope_vals, &algo_steps, &url_match.anchor);
            results.push((url_match.clone(), cov));
        }

        self.doc_coverages.insert(
            uri.to_string(),
            Versioned {
                version,
                data: results.clone(),
            },
        );
        results
    }

    async fn publish_diagnostics(&self, uri: &str, text: &str, version: i32) {
        let validations = self.validate_doc(uri, text, version).await;
        let diagnostics = build_diagnostics(uri, &validations);
        self.client
            .publish_diagnostics(
                uri.parse()
                    .unwrap_or_else(|_| Url::parse("file:///").unwrap()),
                diagnostics,
                None,
            )
            .await;
    }
}

fn build_diagnostics(uri: &str, validations: &[InternalValidation]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for v in validations {
        if matches!(v.result, MatchResult::Exact | MatchResult::Fuzzy) {
            continue;
        }
        let step_label = step_label(&v.step.number);
        let msg = if v.result == MatchResult::NotFound {
            format!(
                "Step {step_label}: not found in algorithm '{}'",
                v.algo_name
            )
        } else {
            format!("Step {step_label}: text differs from spec")
        };
        let end_line = v.step.end_line.unwrap_or(v.step.line);
        let mut diag = Diagnostic {
            range: Range {
                start: Position {
                    line: v.step.line as u32,
                    character: v.step.col_start as u32,
                },
                end: Position {
                    line: end_line as u32,
                    character: v.step.col_end as u32,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("webspec-lens".to_string()),
            message: msg,
            ..Default::default()
        };
        if !v.spec_text.is_empty() {
            diag.related_information = Some(vec![DiagnosticRelatedInformation {
                location: Location {
                    uri: uri
                        .parse()
                        .unwrap_or_else(|_| Url::parse("file:///").unwrap()),
                    range: diag.range,
                },
                message: format!("Expected: {}", v.spec_text),
            }]);
        }
        diagnostics.push(diag);
    }
    diagnostics
}

fn step_label(number: &[u32]) -> String {
    number
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

pub struct Backend {
    state: Arc<State>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(opts) = params.initialization_options {
            if let Some(threshold) = opts.get("fuzzyThreshold").and_then(|v| v.as_f64()) {
                if (0.0..=1.0).contains(&threshold) {
                    *self.state.fuzzy_threshold.lock().await = threshold;
                }
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.state.ensure_pattern().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text.clone();
        let version = params.text_document.version;
        self.state
            .documents
            .insert(uri.clone(), (version, text.clone()));
        self.state.scan_doc(&uri, &text, version).await;
        self.state.publish_diagnostics(&uri, &text, version).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text;
            self.state.documents.insert(uri.clone(), (version, text));

            // Cancel previous debounce
            if let Some((_, old_tx)) = self.state.debounce_tokens.remove(&uri) {
                let _ = old_tx.send(());
            }

            let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(());
            self.state.debounce_tokens.insert(uri.clone(), cancel_tx);

            let state = Arc::clone(&self.state);
            let uri_clone = uri;

            tokio::spawn(async move {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(DEBOUNCE_DELAY_MS)) => {
                        // Fetch latest document text
                        let (version, text) = match state.documents.get(&uri_clone) {
                            Some(entry) => entry.clone(),
                            None => return,
                        };
                        state.scan_doc(&uri_clone, &text, version).await;
                        state.publish_diagnostics(&uri_clone, &text, version).await;
                    }
                    _ = cancel_rx.changed() => {
                        // Cancelled
                    }
                }
            });
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        if let Some((_, tx)) = self.state.debounce_tokens.remove(&uri) {
            let _ = tx.send(());
        }
        self.state.documents.remove(&uri);
        self.state.doc_urls.remove(&uri);
        self.state.doc_validations.remove(&uri);
        self.state.doc_scopes.remove(&uri);
        self.state.doc_coverages.remove(&uri);
        self.state
            .client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
        let pos = params.text_document_position_params.position;

        let (version, text) = match self.state.documents.get(&uri) {
            Some(entry) => entry.clone(),
            None => return Ok(None),
        };

        // Spec URL hover
        let matches = self.state.scan_doc(&uri, &text, version).await;
        if let Some(url_match) =
            find_url_at_position(&matches, pos.line as usize, pos.character as usize)
        {
            if let Some(result) = self
                .state
                .query_spec_cached(&url_match.spec, &url_match.anchor)
            {
                let markdown = build_hover_content(&result);
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: Some(Range {
                        start: Position {
                            line: url_match.line as u32,
                            character: url_match.col_start as u32,
                        },
                        end: Position {
                            line: url_match.line as u32,
                            character: url_match.col_end as u32,
                        },
                    }),
                }));
            }
        }

        // Step comment hover
        let validations = self.state.validate_doc(&uri, &text, version).await;
        for v in &validations {
            if v.step.line != pos.line as usize {
                continue;
            }
            if (pos.character as usize) < v.step.col_start
                || (pos.character as usize) > v.step.col_end
            {
                continue;
            }

            let label = step_label(&v.step.number);
            let md = match v.result {
                MatchResult::Exact => format!("**Step {label}** \u{2014} exact match"),
                MatchResult::Fuzzy => {
                    let mut s = format!("**Step {label}** \u{2014} fuzzy match");
                    if !v.spec_text.is_empty() {
                        s.push_str(&format!("\n\n**Spec:** {}", v.spec_text));
                    }
                    s
                }
                MatchResult::NotFound => {
                    format!("**Step {label}** \u{2014} not found in `{}`", v.algo_name)
                }
                MatchResult::Mismatch => {
                    let mut s = format!("**Step {label}** \u{2014} text differs from spec");
                    if !v.spec_text.is_empty() {
                        s.push_str(&format!("\n\n**Expected:** {}", v.spec_text));
                    }
                    s
                }
            };

            let end_line = v.step.end_line.unwrap_or(v.step.line);
            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: md,
                }),
                range: Some(Range {
                    start: Position {
                        line: v.step.line as u32,
                        character: v.step.col_start as u32,
                    },
                    end: Position {
                        line: end_line as u32,
                        character: v.step.col_end as u32,
                    },
                }),
            }));
        }

        Ok(None)
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.to_string();
        let (version, text) = match self.state.documents.get(&uri) {
            Some(entry) => entry.clone(),
            None => return Ok(None),
        };

        let validations = self.state.validate_doc(&uri, &text, version).await;
        if validations.is_empty() {
            return Ok(None);
        }

        let range_start = params.range.start.line as usize;
        let range_end = params.range.end.line as usize;
        let mut hints = Vec::new();

        for v in &validations {
            if v.step.line < range_start || v.step.line > range_end {
                continue;
            }

            let label_str = step_label(&v.step.number);
            let (hint_label, tooltip) = match v.result {
                MatchResult::Exact => (
                    " \u{2713}",
                    format!("**Step {label_str}** \u{2014} exact match"),
                ),
                MatchResult::Fuzzy => {
                    let mut md = format!("**Step {label_str}** \u{2014} fuzzy match");
                    if !v.spec_text.is_empty() {
                        md.push_str(&format!("\n\n**Spec:** {}", v.spec_text));
                    }
                    (" \u{2713}", md)
                }
                MatchResult::NotFound => (
                    " \u{26a0}",
                    format!(
                        "**Step {label_str}** \u{2014} not found in `{}`",
                        v.algo_name
                    ),
                ),
                MatchResult::Mismatch => {
                    let mut md = format!("**Step {label_str}** \u{2014} text differs from spec");
                    if !v.spec_text.is_empty() {
                        md.push_str(&format!("\n\n**Expected:** {}", v.spec_text));
                    }
                    (" \u{26a0}", md)
                }
            };

            let end_line = v.step.end_line.unwrap_or(v.step.line);
            hints.push(InlayHint {
                position: Position {
                    line: end_line as u32,
                    character: v.step.col_end as u32,
                },
                label: InlayHintLabel::String(hint_label.to_string()),
                kind: Some(match v.result {
                    MatchResult::Exact | MatchResult::Fuzzy => InlayHintKind::TYPE,
                    _ => InlayHintKind::PARAMETER,
                }),
                tooltip: Some(InlayHintTooltip::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: tooltip,
                })),
                padding_left: Some(true),
                padding_right: None,
                text_edits: None,
                data: None,
            });
        }

        Ok(if hints.is_empty() { None } else { Some(hints) })
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri.to_string();
        let (version, text) = match self.state.documents.get(&uri) {
            Some(entry) => entry.clone(),
            None => return Ok(None),
        };

        let coverages = self.state.coverage_doc(&uri, &text, version).await;
        if coverages.is_empty() {
            return Ok(None);
        }

        let mut lenses = Vec::new();
        for (url_match, cov) in &coverages {
            let missing_labels: Vec<String> = cov.missing.iter().map(|s| step_label(s)).collect();

            lenses.push(CodeLens {
                range: Range {
                    start: Position {
                        line: url_match.line as u32,
                        character: 0,
                    },
                    end: Position {
                        line: url_match.line as u32,
                        character: 0,
                    },
                },
                command: Some(Command {
                    title: cov.summary(),
                    command: "webspecLens.showCoverage".to_string(),
                    arguments: Some(vec![
                        serde_json::Value::String(cov.anchor.clone()),
                        serde_json::Value::Number(serde_json::Number::from(cov.total_steps)),
                        serde_json::to_value(&missing_labels).unwrap_or_default(),
                    ]),
                }),
                data: None,
            });
        }

        Ok(if lenses.is_empty() {
            None
        } else {
            Some(lenses)
        })
    }
}

/// Start the LSP server on stdio.
pub async fn serve_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        state: Arc::new(State::new(client)),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
