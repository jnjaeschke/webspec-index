//! State queries (§10): selector resolution and the field, type, member and
//! field-list views. Every answer is assembled from the `state_*` rows of the
//! current snapshots. Only query-time rule packages decode stored `StateSpec`
//! payloads: the sites of snapshots their rules touch are derived in memory.
use crate::db::state::{self as db, StateSnapshot, StoredField, StoredMember, StoredSite};
use crate::state::catalog::StateRule;
use crate::state::ir::StatementSource;
use crate::state::lookup::{fold_name, FieldEntry, Lookup, TypeIndex, TypeNode};
use crate::state::model::{
    AnchorRole, AnchorTarget, ConceptAliasBasis, CoverageCounters, FieldBasis, InitialValue,
    OwnerBasis, OwnerRef, OwnerVia, Site, StateCatalog, StateIssueCode, StateSpec, SuperBasis,
    SuperEdge, TypeExpr, TypeKey, TypeKind,
};
use crate::state::query_algorithm::{algorithm_view, StateAlgorithmResult};
use crate::state::{classify, extract, ir, rules};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

const FIELD_SITE_LIMIT: u32 = 50;
const LIST_LIMIT: u32 = 20;
const LIST_SITES_PER_GROUP: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateQueryOptions {
    pub include_inits: bool,
    pub unclassified: bool,
    pub limit: Option<u32>,
    #[serde(default)]
    pub calls: bool,
    #[serde(default)]
    pub callers: bool,
    #[serde(default)]
    pub opaque: bool,
}

impl Default for StateQueryOptions {
    fn default() -> Self {
        Self {
            include_inits: true,
            unclassified: false,
            limit: None,
            calls: false,
            callers: false,
            opaque: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateErrorCode {
    InvalidSelector,
    SpecNotIndexed,
    NotFound,
    AmbiguousSelector,
    InvalidRules,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateError {
    pub code: StateErrorCode,
    pub message: String,
    pub candidates: Vec<String>,
}

impl StateError {
    pub(crate) fn new(code: StateErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            candidates: Vec::new(),
        }
    }

    fn with_candidates(mut self, candidates: Vec<String>) -> Self {
        self.candidates = candidates;
        self
    }
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StateError {}

/// A database failure while reading the state tables (for example an index
/// built before they existed) means no usable state model.
impl From<anyhow::Error> for StateError {
    fn from(error: anyhow::Error) -> Self {
        Self::new(
            StateErrorCode::SpecNotIndexed,
            format!("state tables unreadable ({error:#}); run `webspec-index update`"),
        )
    }
}

/// The §5 coverage counters for one spec's current snapshot (§10.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateCoverageResult {
    pub spec: String,
    pub snapshot_sha: String,
    pub counters: CoverageCounters,
}

/// What the state views know about a section, with the command that shows it.
/// `query` prints it so that a reader finds the state views from its output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum StateHint {
    Field {
        writes: u32,
        inits: u32,
        command: String,
    },
    Algorithm {
        callers: u32,
        calls: u32,
        command: String,
    },
}

/// The [`StateHint`] for `spec#anchor`, or `None` when the spec has no state
/// model or the counts are all zero.
pub fn state_hint(
    conn: &Connection,
    spec: &str,
    anchor: &str,
    is_algorithm: bool,
) -> anyhow::Result<Option<StateHint>> {
    let snapshots = db::state_snapshots(conn)?;
    let own: Vec<i64> = snapshots
        .iter()
        .filter(|s| s.spec == spec)
        .map(|s| s.id)
        .collect();
    if own.is_empty() {
        return Ok(None);
    }
    let ids: Vec<i64> = snapshots.iter().map(|s| s.id).collect();
    let selector = format!("{spec}#{anchor}");
    let hint = if is_algorithm {
        let (callers, calls) = db::algorithm_call_counts(conn, &ids, &own, spec, anchor)?;
        let flag = if callers > 0 { "--callers" } else { "--calls" };
        StateHint::Algorithm {
            callers,
            calls,
            command: format!("webspec-index state {selector} {flag}"),
        }
    } else {
        let (writes, inits) = db::field_write_counts(conn, &ids, spec, anchor)?;
        StateHint::Field {
            writes,
            inits,
            command: format!("webspec-index state {selector}"),
        }
    };
    let empty = match hint {
        StateHint::Field { writes, inits, .. } => writes == 0 && inits == 0,
        StateHint::Algorithm { callers, calls, .. } => callers == 0 && calls == 0,
    };
    Ok((!empty).then_some(hint))
}

/// Load the coverage counters for the current snapshot of `spec` (§10.5).
/// Returns `StateErrorCode::SpecNotIndexed` when `spec` has no indexed snapshot
/// with a state model.
pub fn coverage(conn: &Connection, spec: &str) -> Result<StateCoverageResult, StateError> {
    use rusqlite::OptionalExtension;
    let spec = spec.trim().to_uppercase();
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT s.sha, c.coverage_json \
             FROM snapshots s \
             JOIN specs sp ON sp.id = s.spec_id \
             JOIN state_coverage c ON c.snapshot_id = s.id \
             WHERE sp.name = ?1 AND s.pr_number IS NULL AND s.sha LIKE 'hash:%' \
             ORDER BY s.id DESC LIMIT 1",
            [&spec],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| StateError::from(anyhow::anyhow!(e)))?;
    match row {
        Some((snapshot_sha, coverage_json)) => {
            let counters: CoverageCounters = serde_json::from_str(&coverage_json)
                .map_err(|e| StateError::from(anyhow::anyhow!(e)))?;
            Ok(StateCoverageResult {
                spec,
                snapshot_sha,
                counters,
            })
        }
        None => Err(StateError::new(
            StateErrorCode::SpecNotIndexed,
            format!("spec {spec} is not indexed; run `webspec-index update`"),
        )),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerInfo {
    pub r#type: String,
    pub name: Option<String>,
    pub via: OwnerVia,
    pub indexed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    pub spec: String,
    pub anchor: String,
    pub name: String,
    pub url: String,
    pub owners: Vec<OwnerInfo>,
    pub owner_hint: Option<String>,
    pub owner_basis: Option<OwnerBasis>,
    pub field_basis: Option<FieldBasis>,
    pub declared_type: TypeExpr,
    pub initial: Option<InitialValue>,
    pub declaration: Option<DeclarationInfo>,
}

/// `section` is `SPEC#anchor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclarationInfo {
    pub section: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteInfo {
    pub spec: String,
    pub subject: String,
    pub step_path: Option<String>,
    pub step_id: Option<String>,
    pub context: String,
    pub role: Option<String>,
    pub op: String,
    pub receiver: String,
    pub target: String,
    pub value: Option<String>,
    pub text: String,
    pub basis: String,
    pub constructed: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteGroup {
    pub count: u32,
    pub items: Vec<SiteInfo>,
}

/// `path` holds type names from the queried type to the owner, e.g. `["Element", "Node"]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundOn {
    pub r#type: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusCounts {
    pub writes: u32,
    pub inits: u32,
    pub unclassified: u32,
    pub possible_unlinked: u32,
    pub reads: u32,
    #[serde(default)]
    pub declared: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Semantics {
    May,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateStatus {
    pub state: StatusState,
    pub semantics: Semantics,
    pub coverage: Coverage,
    pub issues: Vec<StateIssueCode>,
    pub counts: StatusCounts,
}

impl StateStatus {
    pub(crate) fn new(
        complete: bool,
        issues: BTreeSet<StateIssueCode>,
        counts: StatusCounts,
    ) -> Self {
        Self {
            state: StatusState::Ready,
            semantics: Semantics::May,
            coverage: if complete {
                Coverage::Complete
            } else {
                Coverage::Partial
            },
            issues: issues.into_iter().collect(),
            counts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFieldResult {
    pub field: FieldInfo,
    pub found_on: Option<FoundOn>,
    pub writes: Vec<SiteInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inits: Option<Vec<SiteInfo>>,
    pub unclassified: SiteGroup,
    pub possible_unlinked: Vec<SiteInfo>,
    /// Stage C: reflection and rule-declared writes.
    pub declared: Vec<SiteInfo>,
    /// The content attribute an IDL attribute reflects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflects: Option<ReflectsInfo>,
    pub status: StateStatus,
}

/// `content_attribute` is `SPEC#anchor`; `basis` is `[Reflect…]` or `prose`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectsInfo {
    pub content_attribute: Option<String>,
    pub name: String,
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldRow {
    pub spec: String,
    pub anchor: String,
    pub name: String,
    pub declared_type: TypeExpr,
    pub initial: Option<InitialValue>,
    pub basis: String,
    pub writes: u32,
    pub inits: u32,
    pub unclassified: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberRow {
    pub spec: String,
    pub anchor: String,
    pub name: String,
    pub adds: u32,
    pub removes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorInfo {
    pub spec: String,
    pub anchor: String,
    pub role: AnchorRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncludeInfo {
    pub name: String,
    pub spec: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InheritedFields {
    pub from: String,
    pub name: String,
    pub fields: Vec<FieldRow>,
    pub dfn_for_only: Vec<FieldRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTypeResult {
    pub key: String,
    pub name: String,
    pub kind: TypeKind,
    pub anchors: Vec<AnchorInfo>,
    pub supertypes: Vec<String>,
    pub includes: Vec<IncludeInfo>,
    pub fields: Vec<FieldRow>,
    pub dfn_for_only: Vec<FieldRow>,
    pub members: Vec<MemberRow>,
    pub inherited: Vec<InheritedFields>,
    pub truncated: BTreeMap<String, u32>,
    pub status: StateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    pub spec: String,
    pub anchor: String,
    pub name: String,
    pub set: String,
    pub set_name: Option<String>,
    pub declaration: DeclarationInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMemberResult {
    pub member: MemberInfo,
    pub adds: SiteGroup,
    pub removes: SiteGroup,
    pub unclassified: SiteGroup,
    pub status: StateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldListEntry {
    pub row: FieldRow,
    pub owners: Vec<OwnerInfo>,
    pub found_on: Option<FoundOn>,
    pub writes: Vec<SiteInfo>,
    pub inits: Vec<SiteInfo>,
    pub unclassified: Vec<SiteInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFieldListResult {
    pub selector: String,
    pub total: u32,
    pub fields: Vec<FieldListEntry>,
    pub status: StateStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum StateResponse {
    Field(StateFieldResult),
    Type(StateTypeResult),
    Member(StateMemberResult),
    Fields(StateFieldListResult),
    Algorithm(StateAlgorithmResult),
}

/// Resolve `selector` (§10.1) against the current snapshots' state rows.
pub fn query(
    conn: &Connection,
    selector: &str,
    options: &StateQueryOptions,
) -> Result<StateResponse, StateError> {
    query_with_rules(conn, selector, options, &StateCatalog::default())
}

/// [`query`] with the rules of `extra` applied on top of the stored model
/// (§8.3): each in-scope snapshot a rule may match gets its sites re-derived
/// in memory, so the answer adds rule sites and drops the sites they supersede.
pub fn query_with_rules(
    conn: &Connection,
    selector: &str,
    options: &StateQueryOptions,
    extra: &StateCatalog,
) -> Result<StateResponse, StateError> {
    let selector = selector.trim();
    let parsed = parse_selector(selector)?;
    let snapshots = db::state_snapshots(conn)?;
    if snapshots.is_empty() {
        return Err(StateError::new(
            StateErrorCode::SpecNotIndexed,
            "no indexed spec has a state model; run `webspec-index update`",
        ));
    }
    let scope = Scope::load(conn, snapshots, options, extra)?;
    match parsed {
        Selector::Anchor { spec, anchor } => scope.anchor_view(&spec, &anchor),
        Selector::SpecGlob { spec, pattern } => scope.spec_glob(selector, &spec, &pattern),
        Selector::Type(name) => {
            let key = scope.types.resolve(&name)?;
            Ok(StateResponse::Type(scope.type_view(&key)?))
        }
        Selector::TypeField { ty, field } => scope.type_field(&ty, &field),
        Selector::TypeGlob { ty, pattern } => scope.type_glob(selector, &ty, &pattern),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Selector {
    Anchor { spec: String, anchor: String },
    SpecGlob { spec: String, pattern: String },
    Type(String),
    TypeField { ty: String, field: String },
    TypeGlob { ty: String, pattern: String },
}

fn parse_selector(selector: &str) -> Result<Selector, StateError> {
    let invalid = || {
        StateError::new(
            StateErrorCode::InvalidSelector,
            format!("invalid state selector `{selector}`; expected SPEC#anchor, TYPE, TYPE.FIELD, TYPE.GLOB or SPEC#GLOB"),
        )
    };
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(invalid());
    }
    if selector.starts_with("http://") || selector.starts_with("https://") {
        let (spec, anchor, _) = crate::parse_spec_anchor(selector)
            .map_err(|error| StateError::new(StateErrorCode::InvalidSelector, error.to_string()))?;
        return Ok(Selector::Anchor { spec, anchor });
    }
    if let Some((spec, anchor)) = selector.split_once('#') {
        let (spec, anchor) = (spec.trim().to_uppercase(), anchor.trim());
        if spec.is_empty() || anchor.is_empty() {
            return Err(invalid());
        }
        return Ok(if anchor.contains('*') {
            Selector::SpecGlob {
                spec,
                pattern: glob_to_like(anchor),
            }
        } else {
            Selector::Anchor {
                spec,
                anchor: anchor.to_string(),
            }
        });
    }
    if let Some((ty, field)) = selector.split_once('.') {
        let (ty, field) = (ty.trim().to_string(), field.trim());
        if ty.is_empty() || field.is_empty() {
            return Err(invalid());
        }
        return Ok(if field.contains('*') {
            Selector::TypeGlob {
                ty,
                pattern: glob_to_like(field),
            }
        } else {
            Selector::TypeField {
                ty,
                field: field.to_string(),
            }
        });
    }
    Ok(Selector::Type(selector.to_string()))
}

fn glob_to_like(glob: &str) -> String {
    glob.replace('*', "%")
}

/// A `LIKE` pattern matching every name whose `fold_name` equals `folded`:
/// the folded words joined by `%`, with `LIKE` metacharacters widened to `_`.
fn folded_prefilter(folded: &str) -> String {
    let words: Vec<String> = folded
        .split(' ')
        .map(|word| word.replace(['%', '_', '`'], "_"))
        .collect();
    format!("%{}%", words.join("%"))
}

/// SQLite `LIKE`: `%` matches any run, `_` one character, ASCII case-insensitive.
fn like_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let text: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut p, mut t) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == '%' {
            backtrack = Some((p, t));
            p += 1;
        } else if p < pattern.len() && (pattern[p] == '_' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if let Some((star, mark)) = backtrack {
            p = star + 1;
            t = mark + 1;
            backtrack = Some((star, mark + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '%')
}

pub(crate) fn anchor_url(base_url: &str, anchor: &str) -> String {
    if base_url.is_empty() {
        String::new()
    } else if base_url.ends_with(".html") {
        format!("{base_url}#{anchor}")
    } else {
        format!("{}/#{anchor}", base_url.trim_end_matches('/'))
    }
}

/// Display name of a type key that has no `state_types` row.
fn fallback_type_name(key: &str) -> String {
    match TypeKey::parse(key) {
        Some(TypeKey::Idl(name)) => name,
        Some(TypeKey::Anchor(target)) => target.anchor,
        None => key.to_string(),
    }
}

fn parse_role(role: &str) -> Option<AnchorRole> {
    Some(match role {
        "defining" => AnchorRole::Defining,
        "partial" => AnchorRole::Partial,
        "concept_alias:name_match" => AnchorRole::ConceptAlias(ConceptAliasBasis::NameMatch),
        "concept_alias:evidence" => AnchorRole::ConceptAlias(ConceptAliasBasis::Evidence),
        "element_definition" => AnchorRole::ElementDefinition,
        _ => return None,
    })
}

fn parse_super_basis(basis: &str) -> Option<SuperBasis> {
    match basis {
        "idl_inheritance" => Some(SuperBasis::IdlInheritance),
        "idl_includes" => Some(SuperBasis::IdlIncludes),
        _ => basis
            .strip_prefix("override:")
            .map(|rule_id| SuperBasis::Override {
                rule_id: rule_id.to_string(),
            }),
    }
}

/// Decode a stored snake_case enum column that may also hold JSON (struct variants).
fn parse_enum<T: serde::de::DeserializeOwned>(value: &str) -> Option<T> {
    serde_json::from_str(value)
        .ok()
        .or_else(|| serde_json::from_value(serde_json::Value::String(value.to_string())).ok())
}

/// Decode an `owner_basis` column value, handling the `override:<rule_id>` form
/// that cannot be decoded by the generic `parse_enum` helper.
fn parse_owner_basis(value: &str) -> Option<crate::state::OwnerBasis> {
    if let Some(rule_id) = value.strip_prefix("override:") {
        return Some(crate::state::OwnerBasis::Override {
            rule_id: rule_id.to_string(),
        });
    }
    parse_enum(value)
}

struct TypeInfo {
    name: String,
    names: BTreeSet<String>,
    kind: Option<TypeKind>,
    anchors: Vec<AnchorInfo>,
    /// `(super_key, basis, spec of the snapshot that stored the edge)`.
    edges: Vec<(String, SuperBasis, String)>,
}

impl TypeInfo {
    /// The canonical key plus the `SPEC#anchor` keys of its anchors: every
    /// `owner_key` a field of this type can be stored under.
    fn owner_keys(&self, key: &str) -> Vec<String> {
        std::iter::once(key.to_string())
            .chain(
                self.anchors
                    .iter()
                    .map(|a| format!("{}#{}", a.spec, a.anchor)),
            )
            .collect()
    }
}

struct TypeCatalog {
    index: TypeIndex,
    info: BTreeMap<String, TypeInfo>,
    by_anchor: HashMap<(String, String), String>,
}

impl TypeCatalog {
    fn load(
        conn: &Connection,
        ids: &[i64],
        spec_of: &HashMap<i64, String>,
    ) -> anyhow::Result<Self> {
        let mut info: BTreeMap<String, TypeInfo> = BTreeMap::new();
        let mut by_anchor = HashMap::new();
        for row in db::load_types(conn, ids)? {
            let spec = spec_of.get(&row.snapshot_id).cloned().unwrap_or_default();
            let entry = info
                .entry(row.type_key.clone())
                .or_insert_with(|| TypeInfo {
                    name: row.name.clone(),
                    names: BTreeSet::new(),
                    kind: None,
                    anchors: Vec::new(),
                    edges: Vec::new(),
                });
            entry.names.insert(row.name.clone());
            if row.role == "defining" {
                entry.name = row.name.clone();
            }
            if entry.kind.is_none() || row.role == "defining" {
                entry.kind = parse_enum(&row.kind).or(entry.kind.take());
            }
            if let Some(role) = parse_role(&row.role).filter(|_| !row.anchor.is_empty()) {
                let target = stored_anchor(&spec, &row.anchor);
                by_anchor.insert(
                    (target.spec.clone(), target.anchor.clone()),
                    row.type_key.clone(),
                );
                entry.anchors.push(AnchorInfo {
                    spec: target.spec,
                    anchor: target.anchor,
                    role,
                });
            }
        }
        for edge in db::load_edges(conn, ids)? {
            let (Some(basis), Some(entry)) =
                (parse_super_basis(&edge.basis), info.get_mut(&edge.sub_key))
            else {
                continue;
            };
            let spec = spec_of.get(&edge.snapshot_id).cloned().unwrap_or_default();
            if !entry
                .edges
                .iter()
                .any(|(key, b, _)| *key == edge.super_key && *b == basis)
            {
                entry.edges.push((edge.super_key, basis, spec));
            }
        }
        let nodes: Vec<TypeNode> = info
            .iter()
            .filter_map(|(key, ty)| {
                Some(TypeNode {
                    key: TypeKey::parse(key)?,
                    name: ty.name.clone(),
                    supertypes: ty
                        .edges
                        .iter()
                        .filter_map(|(target, basis, _)| {
                            Some(SuperEdge {
                                target: TypeKey::parse(target)?,
                                basis: basis.clone(),
                            })
                        })
                        .collect(),
                })
            })
            .collect();
        let aliases: Vec<(TypeKey, TypeKey)> = by_anchor
            .iter()
            .filter_map(|((spec, anchor), key)| {
                Some((
                    TypeKey::Anchor(AnchorTarget {
                        spec: spec.clone(),
                        anchor: anchor.clone(),
                    }),
                    TypeKey::parse(key)?,
                ))
            })
            .collect();
        Ok(Self {
            index: TypeIndex::new(nodes, aliases),
            info,
            by_anchor,
        })
    }

    fn name_of(&self, key: &str) -> String {
        self.info
            .get(key)
            .map(|ty| ty.name.clone())
            .unwrap_or_else(|| fallback_type_name(key))
    }

    fn canonical(&self, key: &str) -> String {
        match TypeKey::parse(key) {
            Some(parsed) => self.index.canonical(&parsed).to_string(),
            None => key.to_string(),
        }
    }

    /// `idl:TYPE` if present, else the one canonical key whose name folds equal.
    fn resolve(&self, name: &str) -> Result<String, StateError> {
        let idl = format!("idl:{name}");
        if self.info.contains_key(&idl) {
            return Ok(idl);
        }
        let folded = fold_name(name);
        let keys: Vec<&String> = self
            .info
            .iter()
            .filter(|(_, ty)| ty.names.iter().any(|n| fold_name(n) == folded))
            .map(|(key, _)| key)
            .collect();
        match keys.as_slice() {
            [] => Err(StateError::new(
                StateErrorCode::NotFound,
                format!("no type named `{name}` in the state model"),
            )),
            [key] => Ok((*key).clone()),
            _ => Err(StateError::new(
                StateErrorCode::AmbiguousSelector,
                format!("`{name}` names several types"),
            )
            .with_candidates(keys.iter().map(|key| type_selector(key)).collect())),
        }
    }

    /// The canonical key followed by its supertypes, BFS, each with its path.
    fn supertypes_bfs(&self, key: &str) -> Vec<(String, Vec<String>)> {
        let Some(parsed) = TypeKey::parse(key) else {
            return vec![(key.to_string(), vec![key.to_string()])];
        };
        self.index
            .supertypes_bfs(&parsed)
            .into_iter()
            .map(|(ty, path)| {
                (
                    ty.to_string(),
                    path.iter().map(TypeKey::to_string).collect(),
                )
            })
            .collect()
    }

    fn owner_keys(&self, key: &str) -> Vec<String> {
        match self.info.get(key) {
            Some(ty) => ty.owner_keys(key),
            None => vec![key.to_string()],
        }
    }
}

/// A type key rendered as a selector: the name for IDL keys, `SPEC#anchor` otherwise.
fn type_selector(key: &str) -> String {
    key.strip_prefix("idl:").unwrap_or(key).to_string()
}

/// Stored anchors are either bare (the snapshot's own spec) or `SPEC#anchor`.
fn stored_anchor(snapshot_spec: &str, stored: &str) -> AnchorTarget {
    match stored.split_once('#') {
        Some((spec, anchor)) if !spec.is_empty() && !anchor.is_empty() => AnchorTarget {
            spec: spec.to_string(),
            anchor: anchor.to_string(),
        },
        _ => AnchorTarget {
            spec: snapshot_spec.to_string(),
            anchor: stored.to_string(),
        },
    }
}

enum StoredOwners {
    Known(Vec<OwnerRef>),
    Unknown(Option<String>),
}

/// A decoded `state_fields` row.
struct Field {
    spec: String,
    anchor: String,
    row: StoredField,
    names: Vec<String>,
    owners: StoredOwners,
}

impl Field {
    fn decode(snapshot_spec: &str, row: StoredField) -> Self {
        let AnchorTarget { spec, anchor } = stored_anchor(snapshot_spec, &row.anchor);
        let names: Vec<String> = serde_json::from_str(&row.names_json).unwrap_or_default();
        let owners = match serde_json::from_str::<serde_json::Value>(&row.owners_json) {
            Ok(serde_json::Value::Object(map)) if map.contains_key("unknown") => {
                StoredOwners::Unknown(map["unknown"].as_str().map(str::to_string))
            }
            Ok(value) => StoredOwners::Known(serde_json::from_value(value).unwrap_or_default()),
            Err(_) => StoredOwners::Known(Vec::new()),
        };
        Self {
            spec,
            anchor,
            row,
            names,
            owners,
        }
    }

    fn key(&self) -> (String, String) {
        (self.spec.clone(), self.anchor.clone())
    }

    fn all_names(&self) -> Vec<String> {
        let mut names = vec![self.row.name.clone()];
        for name in &self.names {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    fn declared_type(&self) -> TypeExpr {
        serde_json::from_str(&self.row.type_json).unwrap_or(TypeExpr::Unknown)
    }

    fn initial(&self) -> Option<InitialValue> {
        self.row
            .initial_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
    }

    fn is_declared(&self) -> bool {
        self.row.field_basis == "declared"
    }

    fn row(&self, counts: &ClassCounts) -> FieldRow {
        FieldRow {
            spec: self.spec.clone(),
            anchor: self.anchor.clone(),
            name: self.row.name.clone(),
            declared_type: self.declared_type(),
            initial: self.initial(),
            basis: self.row.owner_basis.clone().unwrap_or_default(),
            writes: counts.writes,
            inits: counts.inits,
            unclassified: counts.unclassified,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct ClassCounts {
    writes: u32,
    inits: u32,
    unclassified: u32,
    adds: u32,
    removes: u32,
}

impl ClassCounts {
    fn add(&mut self, class: &str, op: &str, count: u32) {
        match class {
            "write" => self.writes += count,
            "init" => self.inits += count,
            "unclassified" => self.unclassified += count,
            _ => {}
        }
        if matches!(class, "write" | "init" | "declared") {
            if removes_member(op) {
                self.removes += count;
            } else {
                self.adds += count;
            }
        }
    }
}

/// A member site's effect: set operations add the member, `unset`/`remove` remove it.
fn removes_member(op: &str) -> bool {
    matches!(op, "unset" | "remove")
}

fn site_target(site: &StoredSite) -> (&str, &str) {
    (
        site.target_spec.as_deref().unwrap_or_default(),
        site.target_anchor.as_deref().unwrap_or_default(),
    )
}

/// A derived site as `state_sites` stores it for a snapshot of `spec`.
fn stored_row(spec: &str, site: Site) -> StoredSite {
    let (target_spec, target_anchor) = match site.target {
        Some(target) => (Some(target.spec), Some(target.anchor)),
        None => (None, None),
    };
    StoredSite {
        spec: spec.to_string(),
        class: site.class.as_str().to_string(),
        target_spec,
        target_anchor,
        op: site.op,
        subject_anchor: format!("{}#{}", site.subject.spec, site.subject.anchor),
        context: site.context,
        role: site.role,
        constructed: site.constructed,
        step_path: site.step_path,
        step_id: site.step_id,
        receiver: site.receiver,
        target_text: site.target_text,
        value_text: site.value_text,
        text: site.text,
        basis: site.basis,
    }
}

/// Whether `rule` can match in `source`: its subject fits and its anchor is
/// linked or its text occurs. Exclusions are left to `rules::apply_rules`.
fn rule_may_match(rule: &StateRule, source: &StatementSource) -> bool {
    let spec = &rule.match_spec;
    if spec.subject.as_ref().is_some_and(|subject| {
        subject.spec != source.subject.spec || subject.anchor != source.subject.anchor
    }) {
        return false;
    }
    match (&spec.anchor, &spec.text) {
        (Some(anchor), _) => source.links.iter().any(|link| {
            link.target.as_ref().is_some_and(|target| {
                target.spec.eq_ignore_ascii_case(&anchor.spec) && target.anchor == anchor.anchor
            })
        }),
        (None, Some(text)) => text.regex().is_match(&source.text),
        (None, None) => false,
    }
}

/// The sites of `state` with `extra`'s rules applied after the grammar and
/// the rules it was indexed with, or `None` when no rule can match. Only the
/// sources a rule may match are parsed again.
fn sites_with_rules(mut state: StateSpec, extra: &StateCatalog) -> Option<Vec<Site>> {
    let bundled = extract::bundled_catalog();
    let reapply_bundled = !bundled.rules.is_empty()
        && state.representation_version == bundled.representation_version();
    let mut touched = HashSet::new();
    let (mut statements, mut occurrences, mut declared) = (Vec::new(), Vec::new(), Vec::new());
    for source in &state.sources {
        if matches!(
            source.context,
            ir::SourceContext::BranchLabel { .. } | ir::SourceContext::Intro { .. }
        ) || !extra.rules.iter().any(|rule| rule_may_match(rule, source))
        {
            continue;
        }
        let mut parsed = ir::parse_source(source);
        let mut found = classify::classify(source, &parsed);
        if reapply_bundled {
            rules::apply_rules(bundled, source, &mut parsed, &mut found, &mut Vec::new());
        }
        rules::apply_rules(extra, source, &mut parsed, &mut found, &mut declared);
        touched.insert(source.id.clone());
        statements.extend(parsed.statements);
        occurrences.extend(found);
    }
    if touched.is_empty() {
        return None;
    }
    state
        .statements
        .retain(|statement| !touched.contains(&statement.source_id));
    state.statements.extend(statements);
    state
        .occurrences
        .retain(|occurrence| !touched.contains(&occurrence.source_id));
    state.occurrences.extend(occurrences);
    state.declared_sites.extend(declared);
    Some(extract::derive_sites(&state))
}

fn site_info(site: &StoredSite) -> SiteInfo {
    SiteInfo {
        spec: site.spec.clone(),
        subject: stored_anchor(&site.spec, &site.subject_anchor).anchor,
        step_path: site.step_path.clone(),
        step_id: site.step_id.clone(),
        context: site.context.clone(),
        role: site.role.clone(),
        op: site.op.clone(),
        receiver: site.receiver.clone(),
        target: site.target_text.clone(),
        value: site.value_text.clone(),
        text: site.text.clone(),
        basis: site.basis.clone(),
        constructed: site.constructed.clone(),
    }
}

/// Sites split by class; `limit` caps the listed items, counts stay totals.
#[derive(Default)]
struct Groups {
    writes: Vec<SiteInfo>,
    inits: Vec<SiteInfo>,
    unclassified: Vec<SiteInfo>,
    declared: Vec<SiteInfo>,
    counts: ClassCounts,
    declared_count: u32,
}

impl Groups {
    fn new(sites: &[StoredSite], limit: usize) -> Self {
        let mut groups = Self::default();
        for site in sites {
            let (list, count) = match site.class.as_str() {
                "write" => (&mut groups.writes, &mut groups.counts.writes),
                "init" => (&mut groups.inits, &mut groups.counts.inits),
                "unclassified" => (&mut groups.unclassified, &mut groups.counts.unclassified),
                "declared" => (&mut groups.declared, &mut groups.declared_count),
                _ => continue,
            };
            *count += 1;
            if list.len() < limit {
                list.push(site_info(site));
            }
        }
        groups
    }

    fn any_prose(&self) -> bool {
        [
            &self.writes,
            &self.inits,
            &self.unclassified,
            &self.declared,
        ]
        .iter()
        .any(|list| list.iter().any(|site| site.context == "prose"))
    }
}

struct Scope<'a> {
    conn: &'a Connection,
    snapshots: Vec<StateSnapshot>,
    ids: Vec<i64>,
    /// Snapshots whose sites are read from `state_sites`: every snapshot but
    /// those in `rule_sites`.
    site_ids: Vec<i64>,
    /// Sites of the snapshots query-time rules may match, derived in memory
    /// and ordered by site id within each snapshot.
    rule_sites: Vec<StoredSite>,
    spec_of: HashMap<i64, String>,
    types: TypeCatalog,
    options: &'a StateQueryOptions,
}

impl<'a> Scope<'a> {
    fn load(
        conn: &'a Connection,
        snapshots: Vec<StateSnapshot>,
        options: &'a StateQueryOptions,
        extra: &StateCatalog,
    ) -> anyhow::Result<Self> {
        let ids: Vec<i64> = snapshots.iter().map(|s| s.id).collect();
        let spec_of: HashMap<i64, String> =
            snapshots.iter().map(|s| (s.id, s.spec.clone())).collect();
        let types = TypeCatalog::load(conn, &ids, &spec_of)?;
        let mut site_ids = ids.clone();
        let mut rule_sites = Vec::new();
        if !extra.rules.is_empty() {
            for snapshot in &snapshots {
                let Some(state) = db::load_state_model(conn, snapshot.id)? else {
                    continue;
                };
                let Some(mut sites) = sites_with_rules(state, extra) else {
                    continue;
                };
                sites.sort_by(|a, b| a.site_id.cmp(&b.site_id));
                sites.dedup_by(|a, b| a.site_id == b.site_id);
                site_ids.retain(|&id| id != snapshot.id);
                rule_sites.extend(
                    sites
                        .into_iter()
                        .map(|site| stored_row(&snapshot.spec, site)),
                );
            }
        }
        Ok(Self {
            conn,
            snapshots,
            ids,
            site_ids,
            rule_sites,
            spec_of,
            types,
            options,
        })
    }

    /// Every site targeting one of `targets`, in `db::sites_for_targets` order.
    fn sites_for_targets(&self, targets: &[(String, String)]) -> anyhow::Result<Vec<StoredSite>> {
        let mut sites = db::sites_for_targets(self.conn, &self.site_ids, targets)?;
        if self.rule_sites.is_empty() {
            return Ok(sites);
        }
        let wanted: HashSet<(&str, &str)> = targets
            .iter()
            .map(|(spec, anchor)| (spec.as_str(), anchor.as_str()))
            .collect();
        sites.extend(
            self.rule_sites
                .iter()
                .filter(|site| wanted.contains(&site_target(site)))
                .cloned(),
        );
        sites.sort_by(|a, b| {
            (site_target(a), &a.spec, &a.subject_anchor, &a.step_path).cmp(&(
                site_target(b),
                &b.spec,
                &b.subject_anchor,
                &b.step_path,
            ))
        });
        Ok(sites)
    }

    fn sites_for_target(&self, spec: &str, anchor: &str) -> anyhow::Result<Vec<StoredSite>> {
        self.sites_for_targets(&[(spec.to_string(), anchor.to_string())])
    }

    fn spec_ids(&self, spec: &str) -> Vec<i64> {
        self.snapshots
            .iter()
            .filter(|s| s.spec == spec)
            .map(|s| s.id)
            .collect()
    }

    fn spec_in_scope(&self, spec: &str) -> bool {
        self.snapshots.iter().any(|s| s.spec == spec)
    }

    fn base_url(&self, spec: &str) -> anyhow::Result<String> {
        if let Some(snapshot) = self.snapshots.iter().find(|s| s.spec == spec) {
            return Ok(snapshot.base_url.clone());
        }
        if let Some(url) = db::spec_base_url(self.conn, spec)? {
            return Ok(url);
        }
        Ok(crate::spec_registry::SpecRegistry::new()
            .infer_base_url_from_spec_name(spec)
            .map(|(url, _)| url)
            .unwrap_or_default())
    }

    fn decode(&self, row: StoredField) -> Field {
        Field::decode(self.snapshot_spec(row.snapshot_id), row)
    }

    fn snapshot_spec(&self, snapshot_id: i64) -> &str {
        self.spec_of.get(&snapshot_id).map_or("", String::as_str)
    }

    fn member_target(&self, member: &StoredMember) -> AnchorTarget {
        stored_anchor(self.snapshot_spec(member.snapshot_id), &member.anchor)
    }

    fn field_limit(&self) -> usize {
        self.options.limit.unwrap_or(FIELD_SITE_LIMIT) as usize
    }

    /// The error for a `SPEC#…` selector whose spec has no state model in scope.
    fn spec_not_in_scope(spec: &str, has_current_snapshot: bool) -> StateError {
        let message = if has_current_snapshot {
            format!("{spec} has no state model; run `webspec-index update -s {spec}`")
        } else {
            format!("{spec} is not indexed")
        };
        StateError::new(StateErrorCode::SpecNotIndexed, message)
    }

    fn anchor_view(&self, spec: &str, anchor: &str) -> Result<StateResponse, StateError> {
        if !self.spec_in_scope(spec) {
            if db::has_current_snapshot(self.conn, spec)? {
                return Err(Self::spec_not_in_scope(spec, true));
            }
            let sites = self.sites_for_target(spec, anchor)?;
            if sites.is_empty() {
                return Err(Self::spec_not_in_scope(spec, false));
            }
            return Ok(StateResponse::Field(self.field_view(
                spec,
                anchor,
                None,
                None,
                sites,
                StateIssueCode::MissingSpec,
            )?));
        }
        let spec_ids = self.spec_ids(spec);
        if let Some(row) = db::fields_by_anchor(self.conn, &spec_ids, spec, anchor)?
            .into_iter()
            .next()
        {
            let field = self.decode(row);
            let sites = self.sites_for_target(spec, anchor)?;
            return Ok(StateResponse::Field(
                self.declared_field_view(field, None, sites)?,
            ));
        }
        if let Some(member) = db::members_by_anchor(self.conn, &spec_ids, spec, anchor)?
            .into_iter()
            .next()
        {
            return Ok(StateResponse::Member(self.member_view(member)?));
        }
        if let Some(key) = self
            .types
            .by_anchor
            .get(&(spec.to_string(), anchor.to_string()))
        {
            return Ok(StateResponse::Type(self.type_view(key)?));
        }
        let sites = self.sites_for_target(spec, anchor)?;
        // An algorithm anchor written as a field keeps the field view.
        let written = sites
            .iter()
            .any(|site| matches!(site.class.as_str(), "write" | "init" | "declared"));
        if !written {
            if let Some(view) =
                algorithm_view(self.conn, &self.snapshots, spec, anchor, self.options)?
            {
                return Ok(StateResponse::Algorithm(view));
            }
        }
        let reflects = self.reflects(spec, anchor)?;
        if !sites.is_empty() || reflects.is_some() {
            let mut view = self.field_view(
                spec,
                anchor,
                None,
                None,
                sites,
                StateIssueCode::FieldNotDeclared,
            )?;
            view.reflects = reflects;
            return Ok(StateResponse::Field(view));
        }
        let mut candidates = Vec::new();
        if let Some(title) = db::section_title(self.conn, &spec_ids, anchor)? {
            let folded = fold_name(&title);
            for row in db::fields_like(self.conn, &self.ids, &folded_prefilter(&folded))? {
                let field = self.decode(row);
                let selector = format!("{}#{}", field.spec, field.anchor);
                if fold_name(&field.row.name) == folded && !candidates.contains(&selector) {
                    candidates.push(selector);
                }
            }
        }
        Err(StateError::new(
            StateErrorCode::NotFound,
            format!(
                "{spec}#{anchor} is not a field, set member or type, and no state site targets it"
            ),
        )
        .with_candidates(candidates))
    }

    /// The content attribute `spec#anchor` reflects, from its `reflect` site.
    fn reflects(&self, spec: &str, anchor: &str) -> anyhow::Result<Option<ReflectsInfo>> {
        let subject = format!("{spec}#{anchor}");
        let ids: Vec<i64> = self
            .spec_ids(spec)
            .into_iter()
            .filter(|id| self.site_ids.contains(id))
            .collect();
        let stored = if ids.is_empty() {
            Vec::new()
        } else {
            db::reflect_sites_for_subject(self.conn, &ids, &subject)?
        };
        let site = stored.into_iter().next().or_else(|| {
            self.rule_sites
                .iter()
                .find(|site| {
                    site.spec == spec
                        && site.subject_anchor == subject
                        && site.class == "declared"
                        && site.op == "reflect"
                })
                .cloned()
        });
        Ok(site.map(|site| ReflectsInfo {
            content_attribute: site
                .target_spec
                .zip(site.target_anchor)
                .map(|(spec, anchor)| format!("{spec}#{anchor}")),
            name: site.target_text,
            basis: site.role.unwrap_or(site.basis),
        }))
    }

    fn declared_field_view(
        &self,
        field: Field,
        found_on: Option<FoundOn>,
        sites: Vec<StoredSite>,
    ) -> Result<StateFieldResult, StateError> {
        let (spec, anchor) = field.key();
        self.field_view(&spec, &anchor, Some(field), found_on, sites, None)
    }

    fn field_view(
        &self,
        spec: &str,
        anchor: &str,
        field: Option<Field>,
        found_on: Option<FoundOn>,
        sites: Vec<StoredSite>,
        issue: impl Into<Option<StateIssueCode>>,
    ) -> Result<StateFieldResult, StateError> {
        let limit = self.field_limit();
        let groups = Groups::new(&sites, limit);
        let mut issues: BTreeSet<StateIssueCode> = issue.into().into_iter().collect();

        let (info, owners_resolved) = self.field_info(spec, anchor, field.as_ref(), &mut issues)?;
        let possible = match &field {
            Some(field) => self.possible_unlinked(field)?,
            None => Vec::new(),
        };
        let reads = db::read_count(self.conn, &self.ids, spec, anchor)?;

        if let Some(field) = &field {
            let codes: Vec<StateIssueCode> =
                serde_json::from_str(&field.row.issues_json).unwrap_or_default();
            issues.extend(codes);
        }
        if groups.counts.unclassified > 0 {
            issues.insert(StateIssueCode::UnclassifiedOccurrence);
        }
        if !possible.is_empty() {
            issues.insert(StateIssueCode::PossibleUnlinkedWrite);
        }
        let possible_count = possible.len() as u32;
        let possible: Vec<SiteInfo> = possible.into_iter().take(limit).collect();
        if groups.any_prose() || possible.iter().any(|site| site.context == "prose") {
            issues.insert(StateIssueCode::OutsideStructure);
        }
        let complete = groups.counts.unclassified == 0 && possible_count == 0 && owners_resolved;
        let counts = StatusCounts {
            writes: groups.counts.writes,
            inits: groups.counts.inits,
            unclassified: groups.counts.unclassified,
            possible_unlinked: possible_count,
            reads,
            declared: groups.declared_count,
        };
        Ok(StateFieldResult {
            field: info,
            found_on,
            writes: groups.writes,
            inits: self.options.include_inits.then_some(groups.inits),
            unclassified: SiteGroup {
                count: groups.counts.unclassified,
                items: if self.options.unclassified {
                    groups.unclassified
                } else {
                    Vec::new()
                },
            },
            possible_unlinked: possible,
            declared: groups.declared,
            reflects: None,
            status: StateStatus::new(complete, issues, counts),
        })
    }

    /// The field's description and whether its owners are all known and indexed.
    fn field_info(
        &self,
        spec: &str,
        anchor: &str,
        field: Option<&Field>,
        issues: &mut BTreeSet<StateIssueCode>,
    ) -> Result<(FieldInfo, bool), StateError> {
        let url = anchor_url(&self.base_url(spec)?, anchor);
        let Some(field) = field else {
            let name = if self.spec_in_scope(spec) {
                db::section_title(self.conn, &self.spec_ids(spec), anchor)?
            } else {
                None
            };
            let info = FieldInfo {
                spec: spec.to_string(),
                anchor: anchor.to_string(),
                name: name.unwrap_or_else(|| anchor.to_string()),
                url,
                owners: Vec::new(),
                owner_hint: None,
                owner_basis: None,
                field_basis: None,
                declared_type: TypeExpr::Unknown,
                initial: None,
                declaration: None,
            };
            return Ok((info, false));
        };
        let (owners, owner_hint, resolved) = self.owner_infos(field, issues);
        let info = FieldInfo {
            spec: field.spec.clone(),
            anchor: field.anchor.clone(),
            name: field.row.name.clone(),
            url,
            owners,
            owner_hint,
            owner_basis: field.row.owner_basis.as_deref().and_then(parse_owner_basis),
            field_basis: parse_enum(&field.row.field_basis),
            declared_type: field.declared_type(),
            initial: field.initial(),
            declaration: field
                .row
                .decl_section
                .as_ref()
                .map(|section| DeclarationInfo {
                    section: format!("{}#{section}", field.spec),
                    text: field.row.decl_text.clone().unwrap_or_default(),
                }),
        };
        Ok((info, resolved))
    }

    /// Owners with canonical keys, the unknown-owner hint, and whether every
    /// owner is known and indexed.
    fn owner_infos(
        &self,
        field: &Field,
        issues: &mut BTreeSet<StateIssueCode>,
    ) -> (Vec<OwnerInfo>, Option<String>, bool) {
        let refs = match &field.owners {
            StoredOwners::Unknown(hint) => {
                issues.insert(StateIssueCode::OwnerUnknown);
                return (Vec::new(), hint.clone(), false);
            }
            StoredOwners::Known(refs) if refs.is_empty() => {
                issues.insert(StateIssueCode::OwnerUnknown);
                return (Vec::new(), None, false);
            }
            StoredOwners::Known(refs) => refs,
        };
        let mut resolved = true;
        let owners = refs
            .iter()
            .map(|owner| {
                let canonical = self.index_canonical(&owner.key);
                let key = canonical.to_string();
                let name = self.types.info.get(&key).map(|ty| ty.name.clone());
                let indexed = name.is_some()
                    || match &canonical {
                        TypeKey::Idl(_) => false,
                        TypeKey::Anchor(target) => self.spec_in_scope(&target.spec),
                    };
                if !indexed {
                    resolved = false;
                    issues.insert(StateIssueCode::MissingSpec);
                }
                OwnerInfo {
                    r#type: key,
                    name,
                    via: owner.via,
                    indexed,
                }
            })
            .collect();
        (owners, None, resolved)
    }

    fn index_canonical(&self, key: &TypeKey) -> TypeKey {
        self.types.index.canonical(key)
    }

    /// Opaque writes whose target text names this field (`'s NAME`, `the NAME`).
    fn possible_unlinked(&self, field: &Field) -> Result<Vec<SiteInfo>, StateError> {
        let names: Vec<String> = field
            .all_names()
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        let alternatives = names
            .iter()
            .map(|name| regex::escape(name))
            .collect::<Vec<_>>()
            .join("|");
        let Ok(regex) = regex::Regex::new(&format!(r"(?i)(?:'s|’s|\bthe) (?:{alternatives})\b"))
        else {
            return Ok(Vec::new());
        };
        let mut sites = db::opaque_writes_like(self.conn, &self.site_ids, &names)?;
        if !self.rule_sites.is_empty() {
            sites.extend(
                self.rule_sites
                    .iter()
                    .filter(|site| site.class == "opaque_write")
                    .cloned(),
            );
            sites.sort_by(|a, b| {
                (&a.spec, &a.subject_anchor, &a.step_path).cmp(&(
                    &b.spec,
                    &b.subject_anchor,
                    &b.step_path,
                ))
            });
        }
        Ok(sites
            .iter()
            .filter(|site| regex.is_match(&site.target_text))
            .map(site_info)
            .collect())
    }

    fn member_view(&self, member: StoredMember) -> Result<StateMemberResult, StateError> {
        let AnchorTarget { spec, anchor } = self.member_target(&member);
        let sites = self.sites_for_target(&spec, &anchor)?;
        let limit = self.field_limit();
        let (mut adds, mut removes, mut unclassified) = (Vec::new(), Vec::new(), Vec::new());
        let mut counts = ClassCounts::default();
        for site in &sites {
            let (list, count) = match site.class.as_str() {
                "unclassified" => (&mut unclassified, &mut counts.unclassified),
                "write" | "init" | "declared" if removes_member(&site.op) => {
                    (&mut removes, &mut counts.removes)
                }
                "write" | "init" | "declared" => (&mut adds, &mut counts.adds),
                _ => continue,
            };
            *count += 1;
            if list.len() < limit {
                list.push(site_info(site));
            }
        }
        let reads = db::read_count(self.conn, &self.ids, &spec, &anchor)?;
        let mut issues = BTreeSet::new();
        if counts.unclassified > 0 {
            issues.insert(StateIssueCode::UnclassifiedOccurrence);
        }
        if [&adds, &removes, &unclassified]
            .iter()
            .any(|list| list.iter().any(|site| site.context == "prose"))
        {
            issues.insert(StateIssueCode::OutsideStructure);
        }
        let set = self.types.canonical(&member.set_key);
        let set_name = self.types.info.get(&set).map(|ty| ty.name.clone());
        Ok(StateMemberResult {
            member: MemberInfo {
                spec: spec.clone(),
                anchor,
                name: member.name,
                set,
                set_name,
                declaration: DeclarationInfo {
                    section: format!("{spec}#{}", member.decl_section),
                    text: member.decl_text,
                },
            },
            adds: SiteGroup {
                count: counts.adds,
                items: adds,
            },
            removes: SiteGroup {
                count: counts.removes,
                items: removes,
            },
            unclassified: SiteGroup {
                count: counts.unclassified,
                items: if self.options.unclassified {
                    unclassified
                } else {
                    Vec::new()
                },
            },
            status: StateStatus::new(
                counts.unclassified == 0,
                issues,
                StatusCounts {
                    writes: counts.adds + counts.removes,
                    inits: 0,
                    unclassified: counts.unclassified,
                    possible_unlinked: 0,
                    reads,
                    declared: 0,
                },
            ),
        })
    }

    /// Fields owned by each type of `bfs`, in BFS order, each field once
    /// (at the first type that owns it).
    fn fields_along(
        &self,
        bfs: &[(String, Vec<String>)],
    ) -> Result<Vec<(usize, Field)>, StateError> {
        let mut key_to_type: HashMap<String, usize> = HashMap::new();
        let mut keys = Vec::new();
        for (position, (ty, _)) in bfs.iter().enumerate() {
            for key in self.types.owner_keys(ty) {
                key_to_type.entry(key.clone()).or_insert(position);
                keys.push(key);
            }
        }
        let mut best: BTreeMap<(String, String), (usize, Field)> = BTreeMap::new();
        for (owner_key, row) in db::fields_by_owner_keys(self.conn, &self.ids, &keys)? {
            let Some(&position) = key_to_type.get(&owner_key) else {
                continue;
            };
            let field = self.decode(row);
            let key = field.key();
            match best.get(&key) {
                Some((existing, _)) if *existing <= position => {}
                _ => {
                    best.insert(key, (position, field));
                }
            }
        }
        let mut fields: Vec<(usize, Field)> = best.into_values().collect();
        fields.sort_by(|(a, fa), (b, fb)| {
            (a, &fa.spec, fold_name(&fa.row.name), &fa.anchor).cmp(&(
                b,
                &fb.spec,
                fold_name(&fb.row.name),
                &fb.anchor,
            ))
        });
        Ok(fields)
    }

    fn counts_for(
        &self,
        targets: &[(String, String)],
    ) -> Result<HashMap<(String, String), ClassCounts>, StateError> {
        let mut counts: HashMap<(String, String), ClassCounts> = HashMap::new();
        for row in db::site_counts_for_targets(self.conn, &self.site_ids, targets)? {
            let entry = counts.entry((row.spec, row.anchor)).or_default();
            entry.add(&row.class, &row.op, row.count);
        }
        if !self.rule_sites.is_empty() {
            let wanted: HashSet<(&str, &str)> = targets
                .iter()
                .map(|(spec, anchor)| (spec.as_str(), anchor.as_str()))
                .collect();
            for site in &self.rule_sites {
                let target = site_target(site);
                if wanted.contains(&target) {
                    let key = (target.0.to_string(), target.1.to_string());
                    counts.entry(key).or_default().add(&site.class, &site.op, 1);
                }
            }
        }
        Ok(counts)
    }

    fn found_on(&self, path: &[String]) -> FoundOn {
        FoundOn {
            r#type: path.last().cloned().unwrap_or_default(),
            path: path.iter().map(|key| self.types.name_of(key)).collect(),
        }
    }

    fn type_view(&self, key: &str) -> Result<StateTypeResult, StateError> {
        let info =
            self.types.info.get(key).ok_or_else(|| {
                StateError::new(StateErrorCode::NotFound, format!("no type {key}"))
            })?;
        let bfs = self.types.supertypes_bfs(key);
        let fields = self.fields_along(&bfs)?;
        let members = db::members_by_set_keys(self.conn, &self.ids, &info.owner_keys(key))?;

        let mut targets: Vec<(String, String)> = fields.iter().map(|(_, f)| f.key()).collect();
        let member_targets: Vec<(String, String)> = members
            .iter()
            .map(|m| {
                let target = self.member_target(m);
                (target.spec, target.anchor)
            })
            .collect();
        targets.extend(member_targets.iter().cloned());
        let counts = self.counts_for(&targets)?;
        let counts_of = |target: &(String, String)| counts.get(target).copied().unwrap_or_default();

        let limit = self.options.limit.map(|l| l as usize);
        let mut truncated = BTreeMap::new();
        let mut cap = |label: String, mut rows: Vec<FieldRow>| {
            if let Some(limit) = limit.filter(|&limit| rows.len() > limit) {
                truncated.insert(label, (rows.len() - limit) as u32);
                rows.truncate(limit);
            }
            rows
        };

        let mut status_counts = StatusCounts::default();
        let mut own_declared = Vec::new();
        let mut own_dfn_for = Vec::new();
        let mut inherited: Vec<InheritedFields> = Vec::new();
        for (position, field) in &fields {
            let row = field.row(&counts_of(&field.key()));
            if *position == 0 {
                status_counts.writes += row.writes;
                status_counts.inits += row.inits;
                status_counts.unclassified += row.unclassified;
                if field.is_declared() {
                    own_declared.push(row);
                } else {
                    own_dfn_for.push(row);
                }
                continue;
            }
            let from = &bfs[*position].0;
            if inherited.last().is_none_or(|group| group.from != *from) {
                inherited.push(InheritedFields {
                    from: from.clone(),
                    name: self.types.name_of(from),
                    fields: Vec::new(),
                    dfn_for_only: Vec::new(),
                });
            }
            let group = inherited.last_mut().expect("pushed above");
            if field.is_declared() {
                group.fields.push(row);
            } else {
                group.dfn_for_only.push(row);
            }
        }
        let own_declared = cap("fields".into(), own_declared);
        let own_dfn_for = cap("dfn_for_only".into(), own_dfn_for);
        let inherited: Vec<InheritedFields> = inherited
            .into_iter()
            .map(|group| InheritedFields {
                fields: cap(format!("inherited:{}", group.from), group.fields),
                dfn_for_only: cap(
                    format!("inherited_dfn_for_only:{}", group.from),
                    group.dfn_for_only,
                ),
                ..group
            })
            .collect();

        let mut member_rows: Vec<MemberRow> = members
            .iter()
            .zip(&member_targets)
            .map(|(member, target)| {
                let counts = counts_of(target);
                MemberRow {
                    spec: target.0.clone(),
                    anchor: target.1.clone(),
                    name: member.name.clone(),
                    adds: counts.adds,
                    removes: counts.removes,
                }
            })
            .collect();
        if let Some(limit) = limit.filter(|&limit| member_rows.len() > limit) {
            truncated.insert("members".into(), (member_rows.len() - limit) as u32);
            member_rows.truncate(limit);
        }

        let mut issues = BTreeSet::new();
        if status_counts.unclassified > 0 {
            issues.insert(StateIssueCode::UnclassifiedOccurrence);
        }
        Ok(StateTypeResult {
            key: key.to_string(),
            name: info.name.clone(),
            kind: info.kind.clone().unwrap_or(TypeKind::Concept),
            anchors: info.anchors.clone(),
            supertypes: self.inheritance_chain(key),
            includes: info
                .edges
                .iter()
                .filter(|(_, basis, _)| *basis == SuperBasis::IdlIncludes)
                .map(|(target, _, spec)| IncludeInfo {
                    name: self.types.name_of(&self.types.canonical(target)),
                    spec: spec.clone(),
                })
                .collect(),
            fields: own_declared,
            dfn_for_only: own_dfn_for,
            members: member_rows,
            inherited,
            truncated,
            status: StateStatus::new(status_counts.unclassified == 0, issues, status_counts),
        })
    }

    /// Names along the `IdlInheritance` chain, nearest first.
    fn inheritance_chain(&self, key: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::from([key.to_string()]);
        let mut current = key.to_string();
        while let Some(parent) = self.types.info.get(&current).and_then(|ty| {
            ty.edges
                .iter()
                .find(|(_, basis, _)| *basis == SuperBasis::IdlInheritance)
                .map(|(target, _, _)| self.types.canonical(target))
        }) {
            if !seen.insert(parent.clone()) {
                break;
            }
            chain.push(self.types.name_of(&parent));
            current = parent;
        }
        chain
    }

    fn type_field(&self, ty: &str, name: &str) -> Result<StateResponse, StateError> {
        let key = self.types.resolve(ty)?;
        let bfs = self.types.supertypes_bfs(&key);
        let fields: Vec<Field> = self
            .fields_along(&bfs)?
            .into_iter()
            .map(|(_, f)| f)
            .collect();
        let entries: Vec<FieldEntry> = fields
            .iter()
            .map(|field| FieldEntry {
                anchor: AnchorTarget {
                    spec: field.spec.clone(),
                    anchor: field.anchor.clone(),
                },
                names: field.all_names(),
                owners: match &field.owners {
                    StoredOwners::Known(refs) => refs.iter().map(|r| r.key.clone()).collect(),
                    StoredOwners::Unknown(_) => Vec::new(),
                },
            })
            .collect();
        let Some(parsed) = TypeKey::parse(&key) else {
            return Err(StateError::new(
                StateErrorCode::NotFound,
                format!("{ty} has no field `{name}`"),
            ));
        };
        match self.types.index.lookup_field(&parsed, name, &entries) {
            Lookup::Found { field, path } => {
                let path: Vec<String> = path.iter().map(TypeKey::to_string).collect();
                let found_on = self.found_on(&path);
                let field = fields
                    .into_iter()
                    .find(|f| f.spec == field.spec && f.anchor == field.anchor)
                    .expect("lookup returns one of the given fields");
                let (spec, anchor) = field.key();
                let sites = self.sites_for_target(&spec, &anchor)?;
                Ok(StateResponse::Field(self.declared_field_view(
                    field,
                    Some(found_on),
                    sites,
                )?))
            }
            Lookup::Ambiguous(matches) => Err(StateError::new(
                StateErrorCode::AmbiguousSelector,
                format!("`{ty}.{name}` matches several fields"),
            )
            .with_candidates(
                matches
                    .iter()
                    .map(|(field, _)| format!("{}#{}", field.spec, field.anchor))
                    .collect(),
            )),
            Lookup::NotFound => Err(StateError::new(
                StateErrorCode::NotFound,
                format!(
                    "{} has no field `{name}`, own or inherited",
                    self.types.name_of(&key)
                ),
            )),
        }
    }

    fn type_glob(
        &self,
        selector: &str,
        ty: &str,
        pattern: &str,
    ) -> Result<StateResponse, StateError> {
        let key = self.types.resolve(ty)?;
        let bfs = self.types.supertypes_bfs(&key);
        let matches: Vec<(Field, Option<FoundOn>)> = self
            .fields_along(&bfs)?
            .into_iter()
            .filter(|(_, field)| {
                like_match(pattern, &field.row.name) || like_match(pattern, &field.anchor)
            })
            .map(|(position, field)| (field, Some(self.found_on(&bfs[position].1))))
            .collect();
        Ok(StateResponse::Fields(self.field_list(selector, matches)?))
    }

    fn spec_glob(
        &self,
        selector: &str,
        spec: &str,
        pattern: &str,
    ) -> Result<StateResponse, StateError> {
        if !self.spec_in_scope(spec) {
            return Err(Self::spec_not_in_scope(
                spec,
                db::has_current_snapshot(self.conn, spec)?,
            ));
        }
        let matches = db::fields_like(self.conn, &self.spec_ids(spec), pattern)?
            .into_iter()
            .map(|row| (self.decode(row), None))
            .collect();
        Ok(StateResponse::Fields(self.field_list(selector, matches)?))
    }

    fn field_list(
        &self,
        selector: &str,
        matches: Vec<(Field, Option<FoundOn>)>,
    ) -> Result<StateFieldListResult, StateError> {
        let total = matches.len() as u32;
        let limit = self.options.limit.unwrap_or(LIST_LIMIT) as usize;
        let mut issues = BTreeSet::new();
        let mut counts = StatusCounts::default();
        let mut entries = Vec::new();
        let listed: Vec<(Field, Option<FoundOn>)> = matches.into_iter().take(limit).collect();
        let targets: Vec<(String, String)> = listed.iter().map(|(field, _)| field.key()).collect();
        let mut sites_by_target: HashMap<(String, String), Vec<StoredSite>> = HashMap::new();
        for site in self.sites_for_targets(&targets)? {
            let target = (
                site.target_spec.clone().unwrap_or_default(),
                site.target_anchor.clone().unwrap_or_default(),
            );
            sites_by_target.entry(target).or_default().push(site);
        }
        for (field, found_on) in listed {
            let sites = sites_by_target.remove(&field.key()).unwrap_or_default();
            let groups = Groups::new(&sites, LIST_SITES_PER_GROUP);
            let (owners, _, _) = self.owner_infos(&field, &mut issues);
            counts.writes += groups.counts.writes;
            counts.inits += groups.counts.inits;
            counts.unclassified += groups.counts.unclassified;
            if groups.any_prose() {
                issues.insert(StateIssueCode::OutsideStructure);
            }
            entries.push(FieldListEntry {
                row: field.row(&groups.counts),
                owners,
                found_on,
                writes: groups.writes,
                inits: if self.options.include_inits {
                    groups.inits
                } else {
                    Vec::new()
                },
                unclassified: groups.unclassified,
            });
        }
        if counts.unclassified > 0 {
            issues.insert(StateIssueCode::UnclassifiedOccurrence);
        }
        Ok(StateFieldListResult {
            selector: selector.to_string(),
            total,
            fields: entries,
            status: StateStatus::new(counts.unclassified == 0, issues, counts),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::testing::{db_with, query_fixture_db as both, QUERY_HTML as HTML};

    #[test]
    fn state_hints_count_writers_callers_and_calls() {
        let conn = db_with(&[("HTML", crate::state::testing::NAV_HTML)]);
        let hint = |anchor, algorithm| state_hint(&conn, "HTML", anchor, algorithm).unwrap();
        assert!(matches!(
            hint("navigate", true),
            Some(StateHint::Algorithm { callers: 1, ref command, .. })
                if command == "webspec-index state HTML#navigate --callers"
        ));
        assert!(matches!(
            hint("location-object-navigate", true),
            Some(StateHint::Algorithm { callers: 1, calls: 1, ref command })
                if command == "webspec-index state HTML#location-object-navigate --callers"
        ));
        assert!(matches!(
            hint("dom-location-assign", true),
            Some(StateHint::Algorithm { callers: 0, calls, ref command })
                if calls > 0 && command == "webspec-index state HTML#dom-location-assign --calls"
        ));
        assert!(matches!(
            hint("ongoing-navigation", false),
            Some(StateHint::Field { writes, ref command, .. })
                if writes > 0 && command == "webspec-index state HTML#ongoing-navigation"
        ));
        assert_eq!(hint("no-such-anchor", false), None);
        assert_eq!(state_hint(&conn, "DOM", "navigate", true).unwrap(), None);
    }

    fn db(specs: &[(&str, &str, &str)]) -> rusqlite::Connection {
        db_with(
            &specs
                .iter()
                .map(|(name, _, html)| (*name, *html))
                .collect::<Vec<_>>(),
        )
    }

    fn field(conn: &rusqlite::Connection, selector: &str) -> StateFieldResult {
        match query(conn, selector, &StateQueryOptions::default()).unwrap() {
            StateResponse::Field(f) => f,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cross_spec_writes_and_inherited_lookup() {
        let conn = both();
        let f = field(&conn, "Element.node document");
        assert_eq!(f.field.anchor, "concept-node-document");
        assert_eq!(f.found_on.as_ref().unwrap().path, ["Element", "Node"]);
        let subjects: Vec<_> = f
            .writes
            .iter()
            .map(|w| {
                format!(
                    "{}#{}:{}",
                    w.spec,
                    w.subject,
                    w.step_path.clone().unwrap_or_default()
                )
            })
            .collect();
        assert_eq!(
            subjects,
            ["DOM#concept-node-adopt:1", "HTML#document-open-steps:2"]
        );
        assert_eq!(f.unclassified.count, 1);
        assert_eq!(f.status.coverage, Coverage::Partial);
        assert!(f
            .status
            .issues
            .contains(&StateIssueCode::UnclassifiedOccurrence));
    }

    #[test]
    fn owner_in_unindexed_spec_is_reported_not_fatal() {
        let conn = db(&[("HTML", "https://html.spec.whatwg.org/", HTML)]);
        let f = field(&conn, "DOM#concept-node-document");
        assert!(f.field.owners.is_empty());
        assert!(f.status.issues.contains(&StateIssueCode::MissingSpec));
        assert_eq!(f.status.coverage, Coverage::Partial);
        assert_eq!(f.writes.len(), 1);
        assert_eq!(f.writes[0].spec, "HTML");
    }

    #[test]
    fn snapshot_without_state_model_is_spec_not_indexed() {
        let conn = both();
        conn.execute("DELETE FROM state_coverage WHERE snapshot_id IN (SELECT s.id FROM snapshots s JOIN specs sp ON sp.id=s.spec_id WHERE sp.name='HTML')", []).unwrap();
        let error = query(
            &conn,
            "HTML#is-initial-about:blank",
            &StateQueryOptions::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, StateErrorCode::SpecNotIndexed);
        assert!(error.message.contains("update -s HTML"));
    }

    #[test]
    fn pr_snapshot_rows_are_ignored() {
        let conn = both();
        conn.execute(
            "UPDATE snapshots SET pr_number = 7 WHERE spec_id = (SELECT id FROM specs WHERE name='HTML')",
            [],
        )
        .unwrap();
        let f = field(&conn, "DOM#concept-node-document");
        assert!(!f.writes.is_empty());
        assert!(f.writes.iter().all(|w| w.spec == "DOM"));
    }

    #[test]
    fn selector_forms() {
        let conn = both();
        assert!(matches!(
            query(
                &conn,
                "https://html.spec.whatwg.org/#is-initial-about:blank",
                &Default::default()
            )
            .unwrap(),
            StateResponse::Field(_)
        ));
        assert!(
            matches!(query(&conn, "document", &Default::default()).unwrap(), StateResponse::Type(t) if t.key == "idl:Document")
        );
        assert!(
            matches!(query(&conn, "Document", &Default::default()).unwrap(), StateResponse::Type(t) if t.includes.is_empty() && t.supertypes == ["Node"])
        );
        assert!(
            matches!(query(&conn, "Document.*initial*", &Default::default()).unwrap(), StateResponse::Fields(l) if l.total == 1)
        );
        assert!(
            matches!(query(&conn, "Document.*nothing*", &Default::default()).unwrap(), StateResponse::Fields(l) if l.total == 0)
        );
        assert!(
            matches!(query(&conn, "HTML#*initial*", &Default::default()).unwrap(), StateResponse::Fields(l) if l.total == 1)
        );
        assert_eq!(
            query(&conn, "  ", &Default::default()).unwrap_err().code,
            StateErrorCode::InvalidSelector
        );
        assert_eq!(
            query(&conn, "HTML#nope", &Default::default())
                .unwrap_err()
                .code,
            StateErrorCode::NotFound
        );
    }

    #[test]
    fn type_view_lists_own_and_inherited_fields_with_counts() {
        let conn = both();
        let StateResponse::Type(t) = query(&conn, "Document", &Default::default()).unwrap() else {
            panic!()
        };
        let own: Vec<_> = t
            .fields
            .iter()
            .map(|r| (r.anchor.as_str(), r.writes))
            .collect();
        assert_eq!(own, [("is-initial-about:blank", 1)]);
        assert_eq!(t.inherited[0].name, "Node");
        assert_eq!(t.inherited[0].fields[0].writes, 2);
    }

    #[test]
    fn no_inits_hides_the_group_and_limit_caps_sites() {
        let conn = both();
        let options = StateQueryOptions {
            include_inits: false,
            unclassified: true,
            limit: Some(1),
            ..StateQueryOptions::default()
        };
        let StateResponse::Field(f) = query(&conn, "DOM#concept-node-document", &options).unwrap()
        else {
            panic!()
        };
        assert!(f.inits.is_none());
        assert_eq!(f.writes.len(), 1);
        assert_eq!(f.unclassified.items.len(), 1);
    }

    /// Unowned field, possible unlinked write, a write to a non-field anchor,
    /// a heading titled like a field, and a concept type that `OTHER` repeats.
    const EXTRA: &str = r##"<div data-algorithm=""><p>To <dfn id="reset-a-document">reset a document</dfn> given <var>document</var>:</p><ol>
<li><p>Set <var>document</var>'s is initial about:blank to the result of something.</p></li>
<li><p>Set <var>x</var>'s <a href="#reset-a-document">reset</a> to 1.</p></li></ol></div>
<p>Such objects have associated <dfn id="such-thing">thing</dfn>.</p>
<h4 id="initial-heading">is initial about:blank</h4>
<p>A <dfn data-dfn-type="dfn" id="concept-widget">widget</dfn> is a thing.</p>
<p>Each <a href="#concept-widget">widget</a> has a <dfn id="widget-size">size</dfn>, which is a number.</p>"##;

    fn extra_db() -> rusqlite::Connection {
        db_with(&[
            ("DOM", crate::state::testing::QUERY_DOM),
            ("HTML", HTML),
            ("EXTRA", EXTRA),
            ("OTHER", EXTRA),
        ])
    }

    #[test]
    fn member_view_splits_adds_and_removes() {
        let conn = both();
        let StateResponse::Member(m) = query(
            &conn,
            "HTML#sandboxed-navigation-browsing-context-flag",
            &Default::default(),
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(m.member.set, "HTML#sandboxing-flag-set");
        assert_eq!(m.member.set_name.as_deref(), Some("sandboxing flag set"));
        assert_eq!(m.member.declaration.section, "HTML#sandboxing");
        let paths = |sites: &[SiteInfo]| -> Vec<String> {
            sites
                .iter()
                .map(|s| s.step_path.clone().unwrap_or_default())
                .collect()
        };
        assert_eq!(m.adds.count, 1);
        assert_eq!(paths(&m.adds.items), ["1"]);
        assert_eq!(m.removes.count, 1);
        assert_eq!(paths(&m.removes.items), ["2"]);
        assert_eq!(m.status.coverage, Coverage::Complete);

        let StateResponse::Type(t) =
            query(&conn, "HTML#sandboxing-flag-set", &Default::default()).unwrap()
        else {
            panic!()
        };
        let rows: Vec<_> = t
            .members
            .iter()
            .map(|r| (r.anchor.as_str(), r.adds, r.removes))
            .collect();
        assert_eq!(rows, [("sandboxed-navigation-browsing-context-flag", 1, 1)]);
    }

    #[test]
    fn opaque_write_naming_the_field_is_a_possible_unlinked_write() {
        let conn = extra_db();
        let f = field(&conn, "HTML#is-initial-about:blank");
        let texts: Vec<_> = f
            .possible_unlinked
            .iter()
            .map(|s| (s.spec.as_str(), s.text.as_str()))
            .collect();
        assert_eq!(
            texts,
            [
                (
                    "EXTRA",
                    "Set *document*'s is initial about:blank to the result of something."
                ),
                (
                    "OTHER",
                    "Set *document*'s is initial about:blank to the result of something."
                ),
            ]
        );
        assert_eq!(f.status.counts.possible_unlinked, 2);
        assert!(f
            .status
            .issues
            .contains(&StateIssueCode::PossibleUnlinkedWrite));
        assert_eq!(f.status.coverage, Coverage::Partial);
        assert!(field(&both(), "HTML#is-initial-about:blank")
            .possible_unlinked
            .is_empty());
    }

    #[test]
    fn sites_on_a_non_field_anchor_give_field_not_declared() {
        let conn = extra_db();
        let f = field(&conn, "EXTRA#reset-a-document");
        assert!(f.status.issues.contains(&StateIssueCode::FieldNotDeclared));
        assert_eq!(f.field.name, "reset a document");
        assert!(f.field.owners.is_empty());
        assert_eq!(f.writes.len(), 1);
        assert_eq!(f.writes[0].text, "Set *x*'s reset to 1.");
        assert_eq!(f.status.coverage, Coverage::Partial);
    }

    #[test]
    fn reflection_answers_on_content_and_idl_attribute_anchors() {
        let conn = crate::state::testing::db_with(&[("HTML", crate::state::reflect::REFLECT_HTML)]);
        let content = field(&conn, "HTML#attr-hyperlink-target");
        assert!(content
            .status
            .issues
            .contains(&StateIssueCode::FieldNotDeclared));
        assert_eq!(content.declared.len(), 1);
        assert_eq!(content.status.counts.declared, 1);
        assert_eq!(content.declared[0].subject, "dom-a-target");
        assert_eq!(content.declared[0].op, "reflect");
        assert!(content.reflects.is_none());
        let idl = field(&conn, "HTML#dom-a-target");
        let reflects = idl.reflects.expect("reflects");
        assert_eq!(reflects.name, "target");
        assert_eq!(
            reflects.content_attribute.as_deref(),
            Some("HTML#attr-hyperlink-target")
        );
        assert_eq!(reflects.basis, "[Reflect]");
        assert!(idl.declared.is_empty());
    }

    #[test]
    fn not_found_suggests_fields_named_like_the_section_title() {
        let conn = extra_db();
        let error = query(&conn, "EXTRA#initial-heading", &Default::default()).unwrap_err();
        assert_eq!(error.code, StateErrorCode::NotFound);
        assert_eq!(error.candidates, ["HTML#is-initial-about:blank"]);
    }

    #[test]
    fn ambiguous_types_and_fields_list_candidate_selectors() {
        let conn = extra_db();
        let error = query(&conn, "widget", &Default::default()).unwrap_err();
        assert_eq!(error.code, StateErrorCode::AmbiguousSelector);
        assert_eq!(
            error.candidates,
            ["EXTRA#concept-widget", "OTHER#concept-widget"]
        );

        let conn = db_with(&[
            ("DOM", crate::state::testing::QUERY_DOM),
            ("HTML", HTML),
            ("EXTRA", HTML),
        ]);
        let error = query(
            &conn,
            "Document.is initial about:blank",
            &Default::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, StateErrorCode::AmbiguousSelector);
        assert_eq!(
            error.candidates,
            [
                "EXTRA#is-initial-about:blank",
                "HTML#is-initial-about:blank"
            ]
        );
    }

    #[test]
    fn unknown_owner_keeps_its_hint() {
        let conn = extra_db();
        let f = field(&conn, "EXTRA#such-thing");
        assert!(f.field.owners.is_empty());
        assert_eq!(f.field.owner_hint.as_deref(), Some("such"));
        assert!(f.status.issues.contains(&StateIssueCode::OwnerUnknown));
        assert_eq!(f.status.coverage, Coverage::Partial);
    }

    #[test]
    fn unknown_spec_and_empty_scope_are_spec_not_indexed() {
        let error = query(&both(), "NOPE#x", &Default::default()).unwrap_err();
        assert_eq!(error.code, StateErrorCode::SpecNotIndexed);
        assert_eq!(error.message, "NOPE is not indexed");

        let error = query(&db_with(&[]), "Document", &Default::default()).unwrap_err();
        assert_eq!(error.code, StateErrorCode::SpecNotIndexed);
        assert!(error
            .message
            .starts_with("no indexed spec has a state model"));
    }

    #[test]
    fn type_view_limit_records_truncated_rows() {
        let conn = both();
        let options = StateQueryOptions {
            limit: Some(0),
            ..Default::default()
        };
        let StateResponse::Type(t) = query(&conn, "Document", &options).unwrap() else {
            panic!()
        };
        assert!(t.fields.is_empty());
        assert!(t.inherited[0].fields.is_empty());
        assert_eq!(
            t.truncated,
            BTreeMap::from([
                ("fields".to_string(), 1),
                ("inherited:idl:Node".to_string(), 1)
            ])
        );
    }

    #[test]
    fn field_list_entries_carry_their_first_sites() {
        let conn = both();
        let StateResponse::Fields(l) = query(&conn, "Element.*", &Default::default()).unwrap()
        else {
            panic!()
        };
        let entries: Vec<_> = l
            .fields
            .iter()
            .map(|e| {
                (
                    e.row.anchor.as_str(),
                    e.writes.len(),
                    e.unclassified.len(),
                    e.row.writes,
                )
            })
            .collect();
        assert_eq!(entries, [("concept-node-document", 2, 1, 2)]);
    }

    #[test]
    fn selector_parsing_order() {
        assert_eq!(
            parse_selector("html#*sandbox*").unwrap(),
            Selector::SpecGlob {
                spec: "HTML".into(),
                pattern: "%sandbox%".into()
            }
        );
        assert_eq!(
            parse_selector("Element.node document").unwrap(),
            Selector::TypeField {
                ty: "Element".into(),
                field: "node document".into()
            }
        );
        assert_eq!(
            parse_selector("HTML#a.b").unwrap(),
            Selector::Anchor {
                spec: "HTML".into(),
                anchor: "a.b".into()
            }
        );
        assert_eq!(
            parse_selector("environment settings object").unwrap(),
            Selector::Type("environment settings object".into())
        );
        assert_eq!(
            parse_selector("#x").unwrap_err().code,
            StateErrorCode::InvalidSelector
        );
    }

    #[test]
    fn like_match_follows_sqlite_semantics() {
        assert!(like_match("%sandbox%", "active Sandboxing flag set"));
        assert!(like_match("a_c", "abc"));
        assert!(!like_match("a_c", "ac"));
        assert!(like_match("%", ""));
        assert!(like_match("%b%b", "abxb"));
        assert!(!like_match("%nothing%", "is initial about:blank"));
    }

    #[cfg(feature = "native")]
    #[test]
    fn query_time_rules_add_sites_and_declarations_are_rejected() {
        use crate::state::testing::{
            base_url, index_offline_with, ADD_RULE_YAML, QUERY_DOM, QUERY_HTML,
        };
        let conn = crate::db::open_in_memory().unwrap();
        for (spec, html) in [("DOM", QUERY_DOM), ("HTML", QUERY_HTML)] {
            index_offline_with(&conn, spec, base_url(spec), html, &StateCatalog::default())
                .unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("state")).unwrap();
        std::fs::write(dir.path().join("state/rules.yaml"), ADD_RULE_YAML).unwrap();
        let extra = crate::state::catalog::load_state_package(dir.path()).unwrap();
        let options = StateQueryOptions::default();
        let StateResponse::Field(plain) = query(&conn, "HTML#open-dialogs-list", &options).unwrap()
        else {
            panic!()
        };
        assert!(plain.writes.is_empty());
        assert_eq!(plain.unclassified.count, 1);
        let StateResponse::Field(ruled) =
            query_with_rules(&conn, "HTML#open-dialogs-list", &options, &extra).unwrap()
        else {
            panic!()
        };
        assert_eq!(ruled.writes.len(), 1);
        assert_eq!(ruled.writes[0].step_path.as_deref(), Some("3"));
        assert_eq!(
            ruled.writes[0].basis,
            "rule:webspec-index/add-to-field-collection"
        );
        assert_eq!(ruled.unclassified.count, 0);
        let StateResponse::Field(other) =
            query_with_rules(&conn, "DOM#concept-node-document", &options, &extra).unwrap()
        else {
            panic!()
        };
        assert_eq!(other.writes.len(), 2);
        let with_fields = crate::state::catalog::load_state_files(&[(
            "state/f.yaml",
            "schema: 1\npackage: p\nfields:\n  - id: f\n    field: HTML#f\n    owner: [HTML#o]\n    expect_text: 'x'\n    reason: r\n",
        )])
        .unwrap();
        assert!(crate::state::catalog::rules_only(&with_fields)
            .unwrap_err()
            .message
            .contains("only rules"));
    }

    #[cfg(feature = "native")]
    #[test]
    fn query_time_rule_reproducing_a_stored_declared_site_lists_it_once() {
        use crate::state::testing::{base_url, index_offline_with, QUERY_HTML};
        let yaml = "schema: 1\npackage: p\nrules:\n  - id: unset\n    match: {text: 'Unset '}\n    emit: {kind: state.write, params: {field: 'HTML#is-initial-about:blank', op: clear}}\n";
        let catalog = crate::state::catalog::load_state_files(&[("state/r.yaml", yaml)]).unwrap();
        let conn = crate::db::open_in_memory().unwrap();
        index_offline_with(&conn, "HTML", base_url("HTML"), QUERY_HTML, &catalog).unwrap();
        let options = StateQueryOptions::default();
        let selector = "HTML#is-initial-about:blank";
        let StateResponse::Field(stored) = query(&conn, selector, &options).unwrap() else {
            panic!()
        };
        assert_eq!(stored.declared.len(), 1);
        let StateResponse::Field(ruled) =
            query_with_rules(&conn, selector, &options, &catalog).unwrap()
        else {
            panic!()
        };
        assert_eq!(ruled.declared.len(), 1);
        assert_eq!(ruled.declared[0].basis, stored.declared[0].basis);
        let rows = |response: StateResponse| {
            let StateResponse::Fields(list) = response else {
                panic!()
            };
            list.fields
                .into_iter()
                .map(|entry| (entry.row.anchor, entry.row.writes, entry.row.inits))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            rows(query_with_rules(&conn, "Document.*", &options, &catalog).unwrap()),
            rows(query(&conn, "Document.*", &options).unwrap())
        );
    }
}
