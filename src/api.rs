//! Request/response entry point shared by every non-CLI client (wasm, editors, bindings).

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use std::cell::RefCell;
use std::collections::HashMap;

use crate::effects;
use crate::model;
use crate::state;

fn default_true() -> bool {
    true
}

fn default_limit() -> u32 {
    20
}

fn default_direction() -> String {
    "outgoing".into()
}

fn default_trace_depth() -> usize {
    6
}

fn default_max_traces() -> usize {
    20
}

fn default_graph_depth() -> usize {
    2
}

fn default_graph_nodes() -> usize {
    150
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Render {
    #[default]
    Markdown,
    Html,
}

#[derive(Debug, Serialize)]
pub struct QueryResponse {
    #[serde(flatten)]
    pub query: effects::QueryWithEffects,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice: Option<Box<state::slice::SliceResult>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Specs,
    Query {
        target: String,
        #[serde(default)]
        effects: bool,
        #[serde(default)]
        render: Render,
        #[serde(default)]
        view: Option<state::slice::ViewRequest>,
    },
    Exists {
        target: String,
    },
    Search {
        query: String,
        #[serde(default)]
        spec: Option<String>,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    Anchors {
        pattern: String,
        #[serde(default)]
        spec: Option<String>,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    List {
        spec: String,
    },
    Refs {
        target: String,
        #[serde(default = "default_direction")]
        direction: String,
        #[serde(default)]
        kind: Option<model::RefKind>,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    Trace {
        from: String,
        to: String,
        #[serde(default = "default_trace_depth")]
        max_depth: usize,
        #[serde(default)]
        kind: Option<model::RefKind>,
        #[serde(default = "default_max_traces")]
        max_traces: usize,
    },
    Graph {
        target: String,
        #[serde(default = "default_direction")]
        direction: String,
        #[serde(default = "default_graph_depth")]
        max_depth: usize,
        #[serde(default = "default_graph_nodes")]
        max_nodes: usize,
        #[serde(default)]
        include: Vec<String>,
        #[serde(default)]
        exclude: Vec<String>,
        #[serde(default)]
        same_spec_only: bool,
    },
    Idl {
        query: String,
        #[serde(default)]
        spec: Option<String>,
        #[serde(default = "default_limit")]
        limit: u32,
    },
    Effects {
        subject: effects::SubjectSelector,
        #[serde(default)]
        filter: Option<effects::EffectFilter>,
    },
    EffectsExplain {
        subject: effects::SubjectSelector,
    },
    EffectsPaths {
        subject: effects::SubjectSelector,
        effect_id: String,
        #[serde(default = "default_effect_paths_limit")]
        limit: u64,
    },
    Flow {
        target: String,
    },
    State {
        selector: String,
        #[serde(default = "default_true")]
        include_inits: bool,
        #[serde(default)]
        unclassified: bool,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        calls: bool,
        #[serde(default)]
        callers: bool,
        #[serde(default)]
        opaque: bool,
    },
    StateCoverage {
        spec: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "result", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum Response {
    Specs {
        specs: Vec<SpecInfo>,
    },
    Query(QueryResponse),
    Exists(model::ExistsResult),
    Search(model::SearchResult),
    Anchors(model::AnchorsResult),
    List {
        spec: String,
        entries: Vec<model::ListEntry>,
    },
    Refs(model::RefsResult),
    Trace(model::TraceResult),
    Graph(model::GraphResult),
    Idl(model::IdlResult),
    Effects(effects::EffectSummaryResult),
    EffectsExplain(effects::ExplainEffectsResult),
    EffectsPaths(effects::ExplainEffectsResult),
    Flow(model::FlowResult),
    StateField(state::query::StateFieldResult),
    StateType(state::query::StateTypeResult),
    StateMember(state::query::StateMemberResult),
    StateFields(state::query::StateFieldListResult),
    StateAlgorithm(state::query_algorithm::StateAlgorithmResult),
    StateCoverage(state::query::StateCoverageResult),
}

#[derive(Debug, Serialize)]
pub struct SpecInfo {
    pub name: String,
    pub base_url: String,
    pub provider: String,
    pub sha: String,
    pub commit_date: String,
}

/// API-level error code. The `Effects` and `Slice` variants carry their inner
/// code with a matching prefix (`effects_`, `slice_`) so validation errors
/// never collide with envelope errors such as `"invalid_request"`; all others
/// serialize as their own snake_case strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiErrorCode {
    InvalidRequest,
    SpecNotIndexed,
    NotFound,
    Effects(effects::RequestErrorCode),
    State(state::query::StateErrorCode),
    Slice(state::slice::SliceErrorCode),
    Internal,
}

impl Serialize for ApiErrorCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ApiErrorCode::InvalidRequest => s.serialize_str("invalid_request"),
            ApiErrorCode::SpecNotIndexed => s.serialize_str("spec_not_indexed"),
            ApiErrorCode::NotFound => s.serialize_str("not_found"),
            ApiErrorCode::Internal => s.serialize_str("internal"),
            ApiErrorCode::Effects(code) => {
                let inner = serde_json::to_value(code).map_err(serde::ser::Error::custom)?;
                let inner = inner.as_str().ok_or_else(|| {
                    serde::ser::Error::custom("effects error code is not a string")
                })?;
                s.serialize_str(&format!("effects_{inner}"))
            }
            ApiErrorCode::State(code) => {
                let inner = serde_json::to_value(code).map_err(serde::ser::Error::custom)?;
                let inner = inner
                    .as_str()
                    .ok_or_else(|| serde::ser::Error::custom("state error code is not a string"))?;
                s.serialize_str(&format!("state_{inner}"))
            }
            ApiErrorCode::Slice(code) => s.serialize_str(&format!("slice_{}", code.as_str())),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub code: ApiErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    fn not_found(message: impl Into<String>) -> Self {
        ApiError {
            code: ApiErrorCode::NotFound,
            message: message.into(),
            details: None,
        }
    }

    fn internal(error: anyhow::Error) -> Self {
        ApiError {
            code: ApiErrorCode::Internal,
            message: error.to_string(),
            details: None,
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        ApiError {
            code: ApiErrorCode::InvalidRequest,
            message: message.into(),
            details: None,
        }
    }
}

impl From<effects::RequestError> for ApiError {
    fn from(e: effects::RequestError) -> Self {
        ApiError {
            code: ApiErrorCode::Effects(e.code),
            message: e.message,
            details: e.details,
        }
    }
}

impl From<state::slice::SliceError> for ApiError {
    fn from(e: state::slice::SliceError) -> Self {
        ApiError {
            code: ApiErrorCode::Slice(e.code),
            message: e.message,
            details: if e.candidates.is_empty() {
                None
            } else {
                Some(serde_json::json!({"candidates": e.candidates}))
            },
        }
    }
}

impl From<state::query::StateError> for ApiError {
    fn from(e: state::query::StateError) -> Self {
        use state::query::StateErrorCode;
        match e.code {
            StateErrorCode::InvalidSelector => ApiError {
                code: ApiErrorCode::InvalidRequest,
                message: e.message,
                details: None,
            },
            StateErrorCode::SpecNotIndexed => ApiError {
                code: ApiErrorCode::SpecNotIndexed,
                message: e.message,
                details: None,
            },
            StateErrorCode::NotFound => ApiError {
                code: ApiErrorCode::NotFound,
                message: e.message,
                details: if e.candidates.is_empty() {
                    None
                } else {
                    Some(serde_json::json!({"candidates": e.candidates}))
                },
            },
            StateErrorCode::AmbiguousSelector => ApiError {
                code: ApiErrorCode::State(StateErrorCode::AmbiguousSelector),
                message: e.message,
                details: Some(serde_json::json!({"candidates": e.candidates})),
            },
            StateErrorCode::InvalidRules => ApiError {
                code: ApiErrorCode::State(StateErrorCode::InvalidRules),
                message: e.message,
                details: None,
            },
        }
    }
}

#[derive(Serialize)]
struct ErrorEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(flatten)]
    error: ApiError,
}

fn resolve_target(target: &str) -> Result<(String, String), ApiError> {
    crate::parse_spec_anchor(target)
        .map(|(spec, anchor, _)| (canonical_spec_name(&spec), anchor))
        .map_err(|e| ApiError::invalid(e.to_string()))
}

fn canonical_spec_name(spec: &str) -> String {
    spec.to_uppercase()
}

fn map_query_error(e: anyhow::Error) -> ApiError {
    let msg = e.to_string();
    if msg.contains("not indexed") {
        ApiError {
            code: ApiErrorCode::SpecNotIndexed,
            message: msg,
            details: None,
        }
    } else {
        ApiError {
            code: ApiErrorCode::Internal,
            message: msg,
            details: None,
        }
    }
}

fn search(
    conn: &Connection,
    query: &str,
    spec: Option<&str>,
    limit: u32,
) -> Result<model::SearchResult, ApiError> {
    let results = match crate::search_sections_fts(conn, query, spec, limit) {
        Ok(entries) => entries,
        Err(e) => {
            if crate::is_fts_syntax_error(&e) {
                if let Some(sanitized) = crate::sanitize_for_fts(query) {
                    match crate::search_sections_fts(conn, &sanitized, spec, limit) {
                        Ok(entries) => entries,
                        Err(e2) if crate::is_fts_syntax_error(&e2) => vec![],
                        Err(e2) => return Err(ApiError::internal(e2.into())),
                    }
                } else {
                    vec![]
                }
            } else {
                return Err(ApiError::internal(e.into()));
            }
        }
    };
    Ok(model::SearchResult {
        query: query.to_string(),
        results,
    })
}

fn anchors(
    conn: &Connection,
    pattern: &str,
    spec: Option<&str>,
    limit: u32,
) -> Result<model::AnchorsResult, ApiError> {
    let sql_pattern = pattern.replace('*', "%");
    let rows = crate::find_anchors_sql(conn, &sql_pattern, spec, None, limit)
        .map_err(ApiError::internal)?;
    let mut seen = std::collections::HashSet::new();
    let entries: Vec<model::AnchorEntry> = rows
        .into_iter()
        .filter(|(anchor, _, _, _)| seen.insert(anchor.clone()))
        .map(
            |(anchor, spec_name, title, section_type)| model::AnchorEntry {
                spec: spec_name,
                anchor,
                title,
                section_type,
            },
        )
        .collect();
    Ok(model::AnchorsResult {
        pattern: pattern.to_string(),
        results: entries,
    })
}

fn list_specs(conn: &Connection) -> Result<Vec<SpecInfo>, ApiError> {
    let rows = crate::db::queries::list_indexed_specs(conn).map_err(ApiError::internal)?;
    Ok(rows
        .into_iter()
        .map(|(name, base_url, provider, sha, commit_date)| SpecInfo {
            name,
            base_url,
            provider,
            sha,
            commit_date,
        })
        .collect())
}

fn cached_effects_request(
    subject: effects::SubjectSelector,
    filter: Option<effects::EffectFilter>,
) -> effects::EffectsRequest {
    effects::EffectsRequest {
        schema_version: effects::EFFECTS_SCHEMA_VERSION,
        subject,
        options: effects::EffectsOptions {
            mode: effects::EffectsMode::Cached,
            ..effects::EffectsOptions::default()
        },
        filter,
    }
}

fn default_effect_paths_limit() -> u64 {
    8
}

fn with_cached_effects(conn: &Connection, query: model::QueryResult) -> effects::QueryWithEffects {
    let request = cached_effects_request(
        effects::SubjectSelector {
            spec: query.spec.clone(),
            anchor: query.anchor.clone(),
            step_path: None,
            step_id: None,
            body_id: None,
        },
        None,
    );
    let summary =
        effects::service::get_cached_effect_preview_on(conn, &request).and_then(|cached| {
            cached
                .map(Ok)
                .unwrap_or_else(|| effects::service::get_effect_summary_on(conn, &request))
        });
    match summary {
        Ok(result) => effects::query::attach_summary(query, result),
        Err(_) => effects::QueryWithEffects {
            query,
            effects: Some(Vec::new()),
            effects_status: Some(effects::query::error_status()),
        },
    }
}

pub fn handle(conn: &Connection, request: Request) -> Result<Response, ApiError> {
    match request {
        Request::Specs => Ok(Response::Specs {
            specs: list_specs(conn)?,
        }),
        Request::Query {
            target,
            effects: with_effects,
            render,
            view,
        } => {
            let (spec, anchor) = resolve_target(&target)?;
            let mut query = crate::query_section_from_conn(conn, &spec, &anchor)
                .map_err(map_query_error)?
                .ok_or_else(|| ApiError::not_found(format!("{spec}#{anchor}")))?;
            let slice = view
                .map(|v| state::slice::apply_view(conn, &mut query, &v).map(Box::new))
                .transpose()
                .map_err(ApiError::from)?;
            let qwe = if with_effects {
                with_cached_effects(conn, query)
            } else {
                effects::QueryWithEffects {
                    query,
                    effects: None,
                    effects_status: None,
                }
            };
            let content_html = if render == Render::Html && qwe.query.section_type == "idl" {
                qwe.query.content.as_deref().map(crate::render::idl_to_html)
            } else if render == Render::Html {
                qwe.query.content.as_deref().map(|md| {
                    let registry = crate::spec_registry::SpecRegistry::new();
                    let indexed_cache = RefCell::new(HashMap::<String, bool>::new());
                    crate::render::markdown_to_html(md, &|url| {
                        let (spec_name, link_anchor) = registry.resolve_url(url)?;
                        let is_indexed = *indexed_cache
                            .borrow_mut()
                            .entry(spec_name.clone())
                            .or_insert_with(|| {
                                crate::db::queries::get_snapshot(conn, &spec_name)
                                    .ok()
                                    .flatten()
                                    .is_some()
                            });
                        if is_indexed {
                            Some(crate::render::LinkTarget {
                                spec: spec_name,
                                anchor: link_anchor,
                            })
                        } else {
                            None
                        }
                    })
                })
            } else {
                None
            };
            Ok(Response::Query(QueryResponse {
                query: qwe,
                content_html,
                slice,
            }))
        }
        Request::Exists { target } => {
            let (spec, anchor) = resolve_target(&target)?;
            crate::check_exists_from_conn(conn, &spec, &anchor)
                .map(Response::Exists)
                .map_err(map_query_error)
        }
        Request::Search { query, spec, limit } => Ok(Response::Search(search(
            conn,
            &query,
            spec.as_deref(),
            limit,
        )?)),
        Request::Anchors {
            pattern,
            spec,
            limit,
        } => Ok(Response::Anchors(anchors(
            conn,
            &pattern,
            spec.as_deref(),
            limit,
        )?)),
        Request::List { spec } => {
            let spec = canonical_spec_name(&spec);
            crate::list_headings_from_conn(conn, &spec)
                .map(|entries| Response::List { spec, entries })
                .map_err(map_query_error)
        }
        Request::Refs {
            target,
            direction,
            kind,
            limit,
        } => {
            let exact = crate::parse_spec_anchor(&target)
                .ok()
                .map(|(s, a, _)| (s, a));
            crate::find_references_from_conn(conn, exact, &target, &direction, limit, kind)
                .map(Response::Refs)
                .map_err(ApiError::internal)
        }
        Request::Trace {
            from,
            to,
            max_depth,
            kind,
            max_traces,
        } => {
            let from = resolve_target(&from)?;
            let to = resolve_target(&to)?;
            crate::find_traces_from_conn(conn, from, to, max_depth, kind, max_traces)
                .map(Response::Trace)
                .map_err(ApiError::internal)
        }
        Request::Graph {
            target,
            direction,
            max_depth,
            max_nodes,
            include,
            exclude,
            same_spec_only,
        } => {
            let (spec, anchor) = resolve_target(&target)?;
            let filters = crate::GraphFilters {
                include,
                exclude,
                same_spec_only,
            };
            crate::build_graph_from_conn(
                conn, &spec, &anchor, &direction, max_depth, max_nodes, &filters,
            )
            .map(Response::Graph)
            .map_err(ApiError::internal)
        }
        Request::Idl { query, spec, limit } => {
            crate::query_idl_from_conn(conn, &query, spec.as_deref(), limit)
                .map(Response::Idl)
                .map_err(ApiError::internal)
        }
        Request::Effects { subject, filter } => {
            let request = cached_effects_request(subject, filter);
            effects::service::get_effect_summary_on(conn, &request)
                .map(Response::Effects)
                .map_err(ApiError::from)
        }
        Request::EffectsExplain { subject } => {
            let request = cached_effects_request(subject, None);
            effects::service::prepared_effect_details_on(conn, &request)
                .map(Response::EffectsExplain)
                .map_err(ApiError::from)
        }
        Request::EffectsPaths {
            subject,
            effect_id,
            limit,
        } => {
            if effect_id.is_empty() || !(1..=128).contains(&limit) {
                return Err(ApiError::invalid(
                    "effect_id must be non-empty and limit must be between 1 and 128",
                ));
            }
            let request = effects::ExplainEffectsRequest {
                schema_version: effects::EFFECTS_SCHEMA_VERSION,
                subject,
                options: effects::EffectsOptions {
                    mode: effects::EffectsMode::Cached,
                    ..effects::EffectsOptions::default()
                },
                filter: Some(effects::EffectFilter {
                    effect_id: Some(effect_id.clone()),
                    ..effects::EffectFilter::default()
                }),
                explanation: effects::ExplanationOptions {
                    limit,
                    ..effects::ExplanationOptions::default()
                },
            };
            let result =
                effects::service::explain_effects_on(conn, &request).map_err(ApiError::from)?;
            if result.effects.is_empty() {
                return Err(ApiError::from(effects::RequestError {
                    code: effects::RequestErrorCode::EffectNotFound,
                    message: format!("effect {effect_id} is not present in this subject"),
                    details: None,
                }));
            }
            Ok(Response::EffectsPaths(result))
        }
        Request::Flow { target } => {
            let (spec, anchor) = resolve_target(&target)?;
            match crate::flow::flow_from_conn(conn, &spec, &anchor).map_err(ApiError::internal)? {
                Some(result) => Ok(Response::Flow(result)),
                None => Err(ApiError::not_found(format!(
                    "no algorithm steps in {spec}#{anchor}"
                ))),
            }
        }
        Request::State {
            selector,
            include_inits,
            unclassified,
            limit,
            calls,
            callers,
            opaque,
        } => {
            let options = state::query::StateQueryOptions {
                include_inits,
                unclassified,
                limit,
                calls,
                callers,
                opaque,
            };
            let response =
                state::query::query(conn, &selector, &options).map_err(ApiError::from)?;
            Ok(match response {
                state::query::StateResponse::Algorithm(r) => Response::StateAlgorithm(r),
                state::query::StateResponse::Field(r) => Response::StateField(r),
                state::query::StateResponse::Type(r) => Response::StateType(r),
                state::query::StateResponse::Member(r) => Response::StateMember(r),
                state::query::StateResponse::Fields(r) => Response::StateFields(r),
            })
        }
        Request::StateCoverage { spec } => {
            let spec = canonical_spec_name(&spec);
            state::query::coverage(conn, &spec)
                .map(Response::StateCoverage)
                .map_err(ApiError::from)
        }
    }
}

pub fn handle_json(conn: &Connection, request_json: &str) -> String {
    let outcome = match serde_json::from_str::<Request>(request_json) {
        Ok(request) => handle(conn, request),
        Err(e) => Err(ApiError {
            code: ApiErrorCode::InvalidRequest,
            message: e.to_string(),
            details: None,
        }),
    };
    match outcome {
        Ok(response) => serde_json::to_string(&response),
        Err(error) => serde_json::to_string(&ErrorEnvelope {
            kind: "error",
            error,
        }),
    }
    .unwrap_or_else(|e| {
        format!(
            r#"{{"type":"error","code":"internal","message":{}}}"#,
            serde_json::Value::String(e.to_string())
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn conn() -> rusqlite::Connection {
        let c = crate::tests::seed_two_specs();
        db::effects::initialize(&c).unwrap();
        c
    }

    fn call(conn: &rusqlite::Connection, json: &str) -> serde_json::Value {
        serde_json::from_str(&handle_json(conn, json)).unwrap()
    }

    #[test]
    fn specs_lists_indexed_snapshots() {
        let c = conn();
        let v = call(&c, r#"{"type":"specs"}"#);
        assert_eq!(v["type"], "specs");
        let names: Vec<&str> = v["result"]["specs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["DOM", "HTML"]);
        assert_eq!(v["result"]["specs"][1]["sha"], "hash:aaaa");
    }

    #[test]
    fn query_accepts_spec_anchor_and_full_url() {
        let c = conn();
        let a = call(&c, r#"{"type":"query","target":"HTML#navigate"}"#);
        let b = call(
            &c,
            r#"{"type":"query","target":"https://html.spec.whatwg.org/#navigate"}"#,
        );
        assert_eq!(a["type"], "query");
        assert_eq!(a["result"]["anchor"], "navigate");
        assert_eq!(a["result"]["anchor"], b["result"]["anchor"]);
        assert!(a["result"].get("effects").is_none());
    }

    #[test]
    fn spec_names_in_targets_are_case_insensitive() {
        let c = conn();
        let v = call(&c, r#"{"type":"query","target":"html#navigate"}"#);
        assert_eq!(v["type"], "query", "{v}");
        assert_eq!(v["result"]["spec"], "HTML");
        let v = call(&c, r#"{"type":"exists","target":"Html#navigate"}"#);
        assert_eq!(v["result"]["exists"], true, "{v}");
    }

    #[test]
    fn query_type_field_is_envelope_not_section_type() {
        let c = conn();
        let v = call(&c, r#"{"type":"query","target":"HTML#navigate"}"#);
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys.iter().filter(|&&k| k == "type").count(), 1);
        assert_eq!(v["type"], "query");
        assert_eq!(v["result"]["type"], "algorithm");
    }

    #[test]
    fn query_missing_section_is_not_found_and_unindexed_spec_is_distinct() {
        let c = conn();
        let v = call(&c, r#"{"type":"query","target":"HTML#nope"}"#);
        assert_eq!(v["type"], "error");
        assert_eq!(v["code"], "not_found");
        let v = call(&c, r#"{"type":"query","target":"FETCH#x"}"#);
        assert_eq!(v["code"], "spec_not_indexed");
    }

    #[test]
    fn query_with_effects_in_cached_mode_reports_status_without_computing() {
        let c = conn();
        let v = call(
            &c,
            r#"{"type":"query","target":"HTML#navigate","effects":true}"#,
        );
        assert_eq!(v["type"], "query");
        assert!(v["result"].get("effects_status").is_some(), "{v}");
    }

    #[test]
    fn search_falls_back_to_sanitized_query_on_fts_syntax_error() {
        let c = conn();
        let v = call(&c, r#"{"type":"search","query":"navigate"}"#);
        assert_eq!(v["type"], "search");
        assert_eq!(v["result"]["results"][0]["anchor"], "navigate");
        let v = call(&c, r#"{"type":"search","query":"navigate AND ("}"#);
        assert_eq!(v["type"], "search", "{v}");
    }

    #[test]
    fn search_treats_colon_as_punctuation_not_column_filter() {
        let c = conn();
        let v = call(&c, r#"{"type":"search","query":"target is:invalid"}"#);
        assert_eq!(v["type"], "search", "{v}");
        assert_eq!(v["result"]["results"][0]["anchor"], "navigate", "{v}");
        let v = call(&c, r#"{"type":"search","query":"target AND ("}"#);
        assert_eq!(v["result"]["results"][0]["anchor"], "navigate", "{v}");
    }

    #[test]
    fn anchors_list_refs_trace_graph_idl_dispatch() {
        let c = conn();
        assert_eq!(
            call(&c, r#"{"type":"anchors","pattern":"nav*"}"#)["result"]["results"][0]["anchor"],
            "navigate"
        );
        assert_eq!(
            call(&c, r#"{"type":"list","spec":"HTML"}"#)["result"]["entries"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let refs = call(
            &c,
            r#"{"type":"refs","target":"DOM#concept-tree","direction":"incoming"}"#,
        );
        assert_eq!(refs["type"], "refs", "{refs}");
        let trace = call(
            &c,
            r#"{"type":"trace","from":"HTML#navigate","to":"DOM#concept-tree"}"#,
        );
        assert!(
            !trace["result"]["traces"].as_array().unwrap().is_empty(),
            "{trace}"
        );
        let graph = call(&c, r#"{"type":"graph","target":"HTML#navigate"}"#);
        assert_eq!(
            graph["result"]["nodes"].as_array().unwrap().len(),
            2,
            "{graph}"
        );
        assert_eq!(
            call(&c, r#"{"type":"idl","query":"Window"}"#)["type"],
            "idl"
        );
        assert_eq!(
            call(&c, r#"{"type":"exists","target":"HTML#navigate"}"#)["result"]["exists"],
            true
        );
    }

    #[test]
    fn effects_requests_use_cached_mode_and_map_errors() {
        let c = conn();
        let v = call(
            &c,
            r#"{"type":"effects","subject":{"spec":"HTML","anchor":"navigate"}}"#,
        );
        assert!(
            v["type"] == "effects" || v["code"].as_str().unwrap_or("").starts_with("analysis"),
            "{v}"
        );
        let v = call(
            &c,
            r#"{"type":"effects_explain","subject":{"spec":"HTML","anchor":"navigate"}}"#,
        );
        assert!(
            v["type"] == "effects_explain" || v["type"] == "error",
            "{v}"
        );
    }

    #[test]
    fn query_render_html_rewrites_indexed_links_only() {
        let c = conn();
        let v = call(
            &c,
            r#"{"type":"query","target":"HTML#navigate","render":"html"}"#,
        );
        let html = v["result"]["content_html"].as_str().unwrap();
        assert!(html.contains(r##"href="#/DOM/concept-tree""##), "{html}");
        assert!(
            html.contains(r#"href="https://www.w3.org/TR/css-grid-1/#grid""#),
            "{html}"
        );
        assert!(v["result"]["content"]
            .as_str()
            .unwrap()
            .contains("**navigate**"));
        let plain = call(&c, r#"{"type":"query","target":"HTML#navigate"}"#);
        assert!(plain["result"].get("content_html").is_none());
    }

    #[test]
    fn malformed_requests_are_invalid_request_errors() {
        let c = conn();
        let v = call(&c, r#"{"type":"query"}"#);
        assert_eq!(v["type"], "error");
        assert_eq!(v["code"], "invalid_request");
        let v = call(&c, r#"{"type":"query","target":"HTML#navigate","bogus":1}"#);
        assert_eq!(v["code"], "invalid_request");
        let v = call(&c, "not json");
        assert_eq!(v["code"], "invalid_request");
    }

    #[test]
    fn api_error_code_effects_serializes_with_prefix() {
        assert_eq!(
            serde_json::to_string(&ApiErrorCode::Effects(
                effects::RequestErrorCode::SubjectNotFound
            ))
            .unwrap(),
            "\"effects_subject_not_found\""
        );
        assert_ne!(
            serde_json::to_string(&ApiErrorCode::Effects(
                effects::RequestErrorCode::InvalidRequest
            ))
            .unwrap(),
            serde_json::to_string(&ApiErrorCode::InvalidRequest).unwrap()
        );
    }

    #[test]
    fn query_result_carries_number_for_heading() {
        let c = conn();
        let v = call(&c, r#"{"type":"query","target":"HTML#browsing"}"#);
        assert_eq!(v["type"], "query", "{v}");
        assert_eq!(v["result"]["number"], "7.4");
    }

    #[test]
    fn query_result_number_absent_for_algorithm() {
        let c = conn();
        let v = call(&c, r#"{"type":"query","target":"HTML#navigate"}"#);
        assert_eq!(v["type"], "query", "{v}");
        assert!(
            v["result"].get("number").is_none(),
            "algorithm has no number: {v}"
        );
    }

    #[test]
    fn list_entries_carry_number() {
        let c = conn();
        let v = call(&c, r#"{"type":"list","spec":"HTML"}"#);
        assert_eq!(v["type"], "list", "{v}");
        let entries = v["result"]["entries"].as_array().unwrap();
        let browsing = entries.iter().find(|e| e["anchor"] == "browsing").unwrap();
        assert_eq!(browsing["number"], "7.4");
    }

    #[test]
    fn flow_returns_nodes_and_edges_for_algorithm_section() {
        let c = conn();
        let v = call(&c, r#"{"type":"flow","target":"HTML#navigate"}"#);
        assert_eq!(v["type"], "flow", "{v}");
        let result = &v["result"];
        assert_eq!(result["spec"], "HTML");
        assert_eq!(result["anchor"], "navigate");

        let nodes = result["nodes"].as_array().unwrap();
        let edges = result["edges"].as_array().unwrap();
        let issues = result["issues"].as_array().unwrap();

        // Expected: "1" (branch), "1.1" (step), "1.2" (terminal), "2" (branch),
        // "2.1" (step with call), "DOM#concept-tree" (external), "3" (terminal)
        assert_eq!(nodes.len(), 7, "unexpected node count: {nodes:?}");
        // 6 flow edges + 1 call edge
        assert_eq!(edges.len(), 6, "unexpected edge count: {edges:?}");
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");

        let node_kinds: Vec<(&str, &str)> = nodes
            .iter()
            .map(|n| (n["id"].as_str().unwrap(), n["kind"].as_str().unwrap()))
            .collect();

        assert!(node_kinds.contains(&("1", "branch")), "{node_kinds:?}");
        assert!(node_kinds.contains(&("1.1", "step")), "{node_kinds:?}");
        assert!(node_kinds.contains(&("1.2", "terminal")), "{node_kinds:?}");
        assert!(node_kinds.contains(&("2", "branch")), "{node_kinds:?}");
        assert!(node_kinds.contains(&("2.1", "step")), "{node_kinds:?}");
        assert!(
            node_kinds.contains(&("DOM#concept-tree", "external")),
            "{node_kinds:?}"
        );
        assert!(node_kinds.contains(&("3", "terminal")), "{node_kinds:?}");

        // Check key edges are present.
        let edge_triples: Vec<(&str, &str, &str)> = edges
            .iter()
            .map(|e| {
                (
                    e["from"].as_str().unwrap(),
                    e["to"].as_str().unwrap(),
                    e["kind"].as_str().unwrap(),
                )
            })
            .collect();
        assert!(
            edge_triples.contains(&("1", "1.1", "then")),
            "{edge_triples:?}"
        );
        assert!(
            edge_triples.contains(&("1", "2", "else")),
            "{edge_triples:?}"
        );
        assert!(
            edge_triples.contains(&("2", "2.1", "then")),
            "{edge_triples:?}"
        );
        assert!(
            edge_triples.contains(&("2.1", "DOM#concept-tree", "call")),
            "{edge_triples:?}"
        );
        assert!(
            edge_triples.contains(&("2.1", "3", "next")),
            "{edge_triples:?}"
        );

        // The 2.1 node should carry a call entry.
        let node_2_1 = nodes.iter().find(|n| n["id"] == "2.1").unwrap();
        assert_eq!(node_2_1["calls"][0]["spec"], "DOM");
        assert_eq!(node_2_1["calls"][0]["anchor"], "concept-tree");
    }

    #[test]
    fn flow_returns_not_found_for_non_algorithm_and_missing() {
        let c = conn();
        // Heading section has no algorithm steps.
        let v = call(&c, r#"{"type":"flow","target":"HTML#browsing"}"#);
        assert_eq!(v["type"], "error", "{v}");
        assert_eq!(v["code"], "not_found", "{v}");
        // Completely missing section.
        let v = call(&c, r#"{"type":"flow","target":"HTML#nope"}"#);
        assert_eq!(v["type"], "error", "{v}");
        assert_eq!(v["code"], "not_found", "{v}");
    }

    const THING: &str = r#"<p>A <dfn id="thing">thing</dfn> is a concept. Each thing has:</p><ul><li><p>A <dfn id="x">x</dfn>.</p></li></ul>"#;

    fn state_conn() -> rusqlite::Connection {
        use crate::state::testing::{db_with, QUERY_DOM, QUERY_HTML};
        db_with(&[
            ("DOM", QUERY_DOM),
            ("HTML", QUERY_HTML),
            ("A", THING),
            ("B", THING),
        ])
    }

    const GO_VIEW: &str = r##"<div class="algorithm"><p>To <dfn id="go">go</dfn> given a <var>foo</var>:</p><ol>
<li><p>Let <var>a</var> be <var>foo</var>.</p></li>
<li><p>Return.</p></li>
<li><p>Set <var>b</var> to <var>a</var>.</p></li></ol></div>
<p>A <dfn id="thing">thing</dfn> is nice.</p>"##;

    #[test]
    fn query_with_a_view_returns_sliced_content_and_the_slice() {
        let conn = crate::state::testing::db_with(&[("HTML", GO_VIEW)]);
        let v = call(
            &conn,
            r#"{"type":"query","target":"HTML#go","view":{"involving":["*foo*"]}}"#,
        );
        assert_eq!(v["type"], "query", "{v}");
        assert!(v["result"]["content"]
            .as_str()
            .unwrap()
            .contains("- [step 2 omitted: no use of *foo*]"));
        assert_eq!(v["result"]["slice"]["algorithm"], "HTML#go");
        assert_eq!(
            v["result"]["slice"]["view"],
            serde_json::json!({"involving": ["foo"], "depth": null})
        );
        assert!(v["result"].get("effects").is_none());
        let html = call(
            &conn,
            r#"{"type":"query","target":"HTML#go","render":"html","view":{"depth":1}}"#,
        );
        assert!(html["result"]["content_html"].as_str().is_some());
        let plain = call(&conn, r#"{"type":"query","target":"HTML#go"}"#);
        assert!(plain["result"].get("slice").is_none());
        assert!(plain["result"]["content"]
            .as_str()
            .unwrap()
            .contains("2. Return."));
    }

    #[test]
    fn view_errors_are_slice_prefixed() {
        let conn = crate::state::testing::db_with(&[("HTML", GO_VIEW)]);
        let v = call(
            &conn,
            r#"{"type":"query","target":"HTML#go","view":{"involving":["nope"]}}"#,
        );
        assert_eq!(
            (v["type"].as_str(), v["code"].as_str()),
            (Some("error"), Some("slice_unknown_variable"))
        );
        assert_eq!(
            v["details"]["candidates"],
            serde_json::json!(["a", "foo", "b"])
        );
        let v = call(
            &conn,
            r#"{"type":"query","target":"HTML#go","view":{"involving":["foo"],"steps":["1"]}}"#,
        );
        assert_eq!(v["code"], "slice_invalid_selector");
        let v = call(
            &conn,
            r#"{"type":"query","target":"HTML#thing","view":{"involving":["foo"]}}"#,
        );
        assert_eq!(v["code"], "slice_not_an_algorithm");
        let v = call(
            &conn,
            r#"{"type":"query","target":"HTML#go","view":{"bogus":1}}"#,
        );
        assert_eq!(v["code"], "invalid_request");
        use crate::state::slice::select::SliceErrorCode;
        for (code, wire) in [
            (SliceErrorCode::InvalidSelector, "slice_invalid_selector"),
            (SliceErrorCode::NotAnAlgorithm, "slice_not_an_algorithm"),
            (SliceErrorCode::UnknownVariable, "slice_unknown_variable"),
            (SliceErrorCode::UnknownStep, "slice_unknown_step"),
            (SliceErrorCode::Unavailable, "slice_unavailable"),
        ] {
            assert_eq!(
                serde_json::to_value(ApiErrorCode::Slice(code)).unwrap(),
                wire
            );
        }
    }

    #[test]
    fn state_requests_dispatch_and_error_codes() {
        let conn = state_conn();
        let v = call(
            &conn,
            r#"{"type":"state","selector":"HTML#is-initial-about:blank"}"#,
        );
        assert_eq!(v["type"], "state_field");
        assert_eq!(v["result"]["field"]["anchor"], "is-initial-about:blank");
        assert!(v["result"].get("inits").is_some());
        let v = call(
            &conn,
            r#"{"type":"state","selector":"HTML#is-initial-about:blank","include_inits":false}"#,
        );
        assert!(v["result"].get("inits").is_none());
        assert_eq!(
            call(&conn, r#"{"type":"state","selector":"Document"}"#)["type"],
            "state_type"
        );
        assert_eq!(
            call(&conn, r#"{"type":"state","selector":"HTML#*initial*"}"#)["type"],
            "state_fields"
        );
        assert_eq!(
            call(&conn, r#"{"type":"state_coverage","spec":"HTML"}"#)["type"],
            "state_coverage"
        );
        let v = call(&conn, r#"{"type":"state","selector":"thing"}"#);
        assert_eq!(v["type"], "error");
        assert_eq!(v["code"], "state_ambiguous_selector");
        assert_eq!(
            v["details"]["candidates"],
            serde_json::json!(["A#thing", "B#thing"])
        );
        assert_eq!(
            call(&conn, r#"{"type":"state","selector":""}"#)["code"],
            "invalid_request"
        );
        assert_eq!(
            call(&conn, r#"{"type":"state","selector":"HTML#nope"}"#)["code"],
            "not_found"
        );
    }

    #[test]
    fn state_algorithm_requests() {
        let conn = crate::state::testing::db_with(&[("HTML", crate::state::testing::NAV_HTML)]);
        let v = call(
            &conn,
            r#"{"type":"state","selector":"HTML#location-object-navigate","calls":true}"#,
        );
        assert_eq!(v["type"], "state_algorithm", "{v}");
        assert_eq!(v["result"]["calls"][0]["confidence"], "exact");
        assert_eq!(v["result"]["calls"][0]["callee"], "HTML#navigate");
        let v = call(
            &conn,
            r#"{"type":"state","selector":"HTML#navigate","callers":true,"limit":1}"#,
        );
        assert_eq!(v["result"]["callers"]["total"], 1);
        assert!(
            call(&conn, r#"{"type":"state","selector":"HTML#navigate"}"#)["result"]
                .get("calls")
                .is_none_or(|c| c.is_null())
        );
    }
}
