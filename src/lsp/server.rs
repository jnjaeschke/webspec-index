//! tower-lsp based Language Server implementation.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use super::hover::build_hover_content;
use crate::analyze::coverage::StepValidation;
use crate::analyze::file::{analyze_file, FileAnalysis, SpecResolver};
use crate::analyze::matcher::MatchResult;
use crate::analyze::scanner::{find_url_at_position, SpecUrl};

use crate::model::QueryResult;

const DEBOUNCE_DELAY_MS: u64 = 300;

/// Versioned cache entry.
#[derive(Clone)]
struct Versioned<T: Clone> {
    version: i32,
    data: T,
}

/// Shared state that can be cloned into spawned tasks via Arc.
struct State {
    client: Client,
    fuzzy_threshold: Mutex<f64>,
    spec_urls: Mutex<Option<Vec<SpecUrl>>>,
    query_cache: DashMap<String, QueryResult>,
    doc_analysis: DashMap<String, Versioned<FileAnalysis>>,
    debounce_tokens: DashMap<String, tokio::sync::watch::Sender<()>>,
    documents: DashMap<String, (i32, String)>,
}

impl State {
    fn new(client: Client) -> Self {
        Self {
            client,
            fuzzy_threshold: Mutex::new(0.85),
            spec_urls: Mutex::new(None),
            query_cache: DashMap::new(),
            doc_analysis: DashMap::new(),
            debounce_tokens: DashMap::new(),
            documents: DashMap::new(),
        }
    }

    async fn ensure_spec_urls(&self) -> Vec<SpecUrl> {
        let mut cached = self.spec_urls.lock().await;
        if let Some(ref urls) = *cached {
            return urls.clone();
        }
        let spec_entries = crate::spec_urls();
        let urls: Vec<SpecUrl> = spec_entries
            .iter()
            .map(|e| SpecUrl {
                spec: e.spec.clone(),
                base_url: e.base_url.clone(),
            })
            .collect();
        *cached = Some(urls.clone());
        urls
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

    async fn analyze_doc(&self, uri: &str, text: &str, version: i32) -> FileAnalysis {
        if let Some(cached) = self.doc_analysis.get(uri) {
            if cached.version == version {
                return cached.data.clone();
            }
        }

        let spec_urls = self.ensure_spec_urls().await;
        let threshold = *self.fuzzy_threshold.lock().await;
        let resolver = LspResolver { state: self };
        let analysis = analyze_file(text, &spec_urls, &resolver, threshold);

        self.doc_analysis.insert(
            uri.to_string(),
            Versioned {
                version,
                data: analysis.clone(),
            },
        );
        analysis
    }

    async fn publish_diagnostics(&self, uri: &str, text: &str, version: i32) {
        let analysis = self.analyze_doc(uri, text, version).await;
        let diagnostics = build_diagnostics(uri, &analysis);
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

/// Spec resolver backed by the LSP's cached DB queries.
struct LspResolver<'a> {
    state: &'a State,
}

impl SpecResolver for LspResolver<'_> {
    fn resolve(&self, spec: &str, anchor: &str) -> Option<String> {
        self.state
            .query_spec_cached(spec, anchor)?
            .content
            .filter(|c| !c.is_empty())
    }
}

fn build_diagnostics(uri: &str, analysis: &FileAnalysis) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for scope in &analysis.scopes {
        for v in &scope.validations {
            if matches!(v.result, MatchResult::Exact | MatchResult::Fuzzy) {
                continue;
            }
            let step_label = step_label(&v.step.number);
            let msg = if v.result == MatchResult::NotFound {
                format!(
                    "Step {step_label}: not found in algorithm '{}'",
                    v.algo_anchor
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

/// Find a step validation at the given cursor position.
fn find_validation_at_position(
    analysis: &FileAnalysis,
    line: usize,
    col: usize,
) -> Option<&StepValidation> {
    for scope in &analysis.scopes {
        for v in &scope.validations {
            if v.step.line != line {
                continue;
            }
            if col >= v.step.col_start && col <= v.step.col_end {
                return Some(v);
            }
        }
    }
    None
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
        self.state.ensure_spec_urls().await;
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
                        let (version, text) = match state.documents.get(&uri_clone) {
                            Some(entry) => entry.clone(),
                            None => return,
                        };
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
        self.state.doc_analysis.remove(&uri);
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

        let analysis = self.state.analyze_doc(&uri, &text, version).await;

        // Spec URL hover
        if let Some(url_match) = find_url_at_position(
            &analysis.url_matches,
            pos.line as usize,
            pos.character as usize,
        ) {
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
        if let Some(v) =
            find_validation_at_position(&analysis, pos.line as usize, pos.character as usize)
        {
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
                    format!("**Step {label}** \u{2014} not found in `{}`", v.algo_anchor)
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

        let analysis = self.state.analyze_doc(&uri, &text, version).await;
        let range_start = params.range.start.line as usize;
        let range_end = params.range.end.line as usize;
        let mut hints = Vec::new();

        for scope in &analysis.scopes {
            for v in &scope.validations {
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
                            v.algo_anchor
                        ),
                    ),
                    MatchResult::Mismatch => {
                        let mut md =
                            format!("**Step {label_str}** \u{2014} text differs from spec");
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
        }

        Ok(if hints.is_empty() { None } else { Some(hints) })
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = params.text_document.uri.to_string();
        let (version, text) = match self.state.documents.get(&uri) {
            Some(entry) => entry.clone(),
            None => return Ok(None),
        };

        let analysis = self.state.analyze_doc(&uri, &text, version).await;
        let mut lenses = Vec::new();

        for scope in &analysis.scopes {
            let cov = match &scope.coverage {
                Some(c) => c,
                None => continue,
            };

            let missing_labels: Vec<String> = cov.missing.iter().map(|s| step_label(s)).collect();

            lenses.push(CodeLens {
                range: Range {
                    start: Position {
                        line: scope.url_match.line as u32,
                        character: 0,
                    },
                    end: Position {
                        line: scope.url_match.line as u32,
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
