//! Reviewed state declarations and rules: the `state/` files of a semantics
//! package (spec §8).

#[cfg(feature = "native")]
use crate::semantics::collect_yaml_files;
use crate::semantics::{
    compile_match, compile_pattern, parse_anchor, parse_yaml_file, sha256_bytes, validate_id,
    CatalogError, CatalogPattern, MatchSpec, RawMatchSpec,
};
use crate::state::model::{
    AnchorTarget, InfraKind, InitialValue, Literal, Primitive, TypeExpr, TypeKey, TypeKind,
    TypeRef, STATE_VERSION,
};
use crate::state::query::{StateError, StateErrorCode};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "native")]
use std::path::Path;

pub const STATE_SCHEMA_VERSION: u32 = 1;

const STATE_DIR: &str = "state/";

/// Reviewed declarations and rules applied at index time.
#[derive(Debug, Clone, Default)]
pub struct StateCatalog {
    pub digest: Option<String>,
    pub types: Vec<TypeDeclaration>,
    pub fields: Vec<FieldDeclaration>,
    pub rules: Vec<StateRule>,
}

impl StateCatalog {
    pub fn representation_version(&self) -> String {
        match &self.digest {
            Some(digest) => format!("{STATE_VERSION}+{digest}"),
            None => STATE_VERSION.to_string(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && !self.has_declarations()
    }

    /// Whether the catalog declares types or fields, which query-time rule
    /// packages may not.
    pub fn has_declarations(&self) -> bool {
        !self.types.is_empty() || !self.fields.is_empty()
    }
}

/// Query-time rule packages add rules only: declarations change the stored
/// model, which is built once at index time.
pub fn rules_only(catalog: &StateCatalog) -> Result<(), StateError> {
    if catalog.has_declarations() {
        return Err(StateError::new(
            StateErrorCode::InvalidRules,
            "query-time rule packages may contain only rules; declarations apply when the bundled catalog is built",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct TypeDeclaration {
    pub public_id: String,
    pub ty: AnchorTarget,
    pub name: Option<String>,
    pub kind: Option<TypeKind>,
    pub alias_of: Option<TypeKey>,
    pub implemented_by: Vec<TypeKey>,
    pub expect_text: CatalogPattern,
    pub reason: String,
    pub source_file: String,
}

#[derive(Debug, Clone)]
pub struct FieldDeclaration {
    pub public_id: String,
    pub field: AnchorTarget,
    pub owner: Option<Vec<TypeKey>>,
    pub ty: Option<TypeExpr>,
    pub initial: Option<InitialValue>,
    pub expect_text: CatalogPattern,
    pub reason: String,
    pub source_file: String,
}

#[derive(Debug, Clone)]
pub struct StateRule {
    pub public_id: String,
    pub match_spec: MatchSpec,
    pub emit: StateEmit,
    pub description: Option<String>,
    pub source_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateEmit {
    Write {
        field: AnchorTarget,
        op: String,
    },
    Init {
        field: AnchorTarget,
    },
    /// `target` and `operand` name capture groups of the rule's `match.text`.
    Mutate {
        op: String,
        target: String,
        operand: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStateFile {
    schema: u32,
    package: String,
    #[serde(default)]
    types: Vec<RawTypeDeclaration>,
    #[serde(default)]
    fields: Vec<RawFieldDeclaration>,
    #[serde(default)]
    rules: Vec<RawStateRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTypeDeclaration {
    id: String,
    #[serde(rename = "type")]
    ty: String,
    name: Option<String>,
    kind: Option<TypeKind>,
    alias_of: Option<String>,
    #[serde(default)]
    implemented_by: Vec<String>,
    expect_text: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFieldDeclaration {
    id: String,
    field: String,
    owner: Option<Vec<String>>,
    /// `Some(Value::Null)` for an explicit `null`, which `type` rejects and
    /// `initial` accepts as a literal.
    #[serde(rename = "type", default, deserialize_with = "present")]
    ty: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    initial: Option<Value>,
    expect_text: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStateRule {
    id: String,
    #[serde(rename = "match")]
    match_spec: RawMatchSpec,
    emit: RawStateEmit,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStateEmit {
    kind: String,
    #[serde(default)]
    params: Map<String, Value>,
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// Load the state layer of one package from `(relative_path, UTF-8 YAML)`
/// pairs. Only paths under `state/` are read; the rest belongs to effects.
pub fn load_state_files(files: &[(&str, &str)]) -> Result<StateCatalog, CatalogError> {
    let mut files: Vec<(&str, &str)> = files
        .iter()
        .copied()
        .filter(|(path, _)| path.starts_with(STATE_DIR))
        .collect();
    if files.is_empty() {
        return Ok(StateCatalog::default());
    }
    files.sort_by(|left, right| left.0.cmp(right.0));
    let mut digest_input = Vec::new();
    for (path, content) in &files {
        if !(path.ends_with(".yaml") || path.ends_with(".yml")) {
            return Err(CatalogError::file(
                *path,
                "state catalog file must end in .yaml or .yml",
            ));
        }
        digest_input.extend_from_slice(path.as_bytes());
        digest_input.push(0);
        digest_input.extend_from_slice(content.as_bytes());
        digest_input.push(0);
    }
    let mut catalog = StateCatalog {
        digest: Some(format!("sha256:{}", sha256_bytes(&digest_input))),
        ..StateCatalog::default()
    };
    let mut package_id: Option<String> = None;
    let mut ids = BTreeSet::new();
    for (file, content) in files {
        let raw: RawStateFile = parse_yaml_file(file, content)?;
        if raw.schema != STATE_SCHEMA_VERSION {
            return Err(CatalogError::file(
                file,
                format!(
                    "unsupported state catalog schema {}; expected {STATE_SCHEMA_VERSION}",
                    raw.schema
                ),
            ));
        }
        let package = match &package_id {
            Some(expected) if *expected != raw.package => {
                return Err(CatalogError::file(
                    file,
                    format!("package is {}, expected {expected}", raw.package),
                ));
            }
            Some(expected) => expected.clone(),
            None => {
                validate_id(&raw.package, "package identifier", file)?;
                package_id = Some(raw.package.clone());
                raw.package
            }
        };
        for raw_type in raw.types {
            let public_id = declaration_id(&package, &raw_type.id, &mut ids, file)?;
            catalog.types.push(compile_type(raw_type, public_id, file)?);
        }
        for raw_field in raw.fields {
            let public_id = declaration_id(&package, &raw_field.id, &mut ids, file)?;
            catalog
                .fields
                .push(compile_field(raw_field, public_id, file)?);
        }
        for raw_rule in raw.rules {
            let public_id = declaration_id(&package, &raw_rule.id, &mut ids, file)?;
            catalog.rules.push(compile_rule(raw_rule, public_id, file)?);
        }
    }
    Ok(catalog)
}

/// Load the `state/` directory of a package directory; a package without one
/// has an empty state catalog.
#[cfg(feature = "native")]
pub fn load_state_package(dir: impl AsRef<Path>) -> Result<StateCatalog, CatalogError> {
    let root = dir.as_ref();
    let state_dir = root.join(STATE_DIR.trim_end_matches('/'));
    let mut paths = Vec::new();
    if state_dir.is_dir() {
        collect_yaml_files(root, &state_dir, &mut paths)?;
    }
    let mut files = Vec::with_capacity(paths.len());
    for (relative, absolute) in paths {
        let content = std::fs::read_to_string(&absolute).map_err(|error| {
            CatalogError::file(relative.clone(), format!("cannot read UTF-8 YAML: {error}"))
        })?;
        files.push((relative, content));
    }
    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    load_state_files(&borrowed)
}

/// Combine the state catalogs of several packages. Public ids must be unique;
/// the digest covers every contributing digest, independent of order.
pub fn merge(
    catalogs: impl IntoIterator<Item = StateCatalog>,
) -> Result<StateCatalog, CatalogError> {
    let mut merged = StateCatalog::default();
    let mut digests = Vec::new();
    for catalog in catalogs {
        digests.extend(catalog.digest);
        merged.types.extend(catalog.types);
        merged.fields.extend(catalog.fields);
        merged.rules.extend(catalog.rules);
    }
    let mut seen = BTreeSet::new();
    let ids = merged
        .types
        .iter()
        .map(|d| (&d.public_id, &d.source_file))
        .chain(merged.fields.iter().map(|d| (&d.public_id, &d.source_file)))
        .chain(merged.rules.iter().map(|d| (&d.public_id, &d.source_file)));
    for (id, file) in ids {
        if !seen.insert(id) {
            return Err(CatalogError::file(
                file,
                format!("duplicate state declaration ID {id}"),
            ));
        }
    }
    digests.sort();
    merged.digest = match digests.len() {
        0 => None,
        1 => digests.pop(),
        _ => {
            let mut input = Vec::new();
            for digest in &digests {
                input.extend_from_slice(digest.as_bytes());
                input.push(0);
            }
            Some(format!("sha256:{}", sha256_bytes(&input)))
        }
    };
    Ok(merged)
}

fn declaration_id(
    package: &str,
    id: &str,
    ids: &mut BTreeSet<String>,
    file: &str,
) -> Result<String, CatalogError> {
    validate_id(id, "declaration ID", file)?;
    if !ids.insert(id.to_owned()) {
        return Err(CatalogError::file(
            file,
            format!("duplicate package-local declaration ID {id}"),
        ));
    }
    Ok(format!("{package}/{id}"))
}

fn anchor_target(value: &str, file: &str, location: &str) -> Result<AnchorTarget, CatalogError> {
    let anchor = parse_anchor(value, file, location)?;
    Ok(AnchorTarget {
        spec: anchor.spec,
        anchor: anchor.anchor,
    })
}

/// `idl:Name` or `SPEC#anchor`.
fn type_key(value: &str, file: &str, location: &str) -> Result<TypeKey, CatalogError> {
    if value.starts_with("idl:") {
        return TypeKey::parse(value).ok_or_else(|| {
            CatalogError::file(file, format!("malformed type key {value:?}")).at(location)
        });
    }
    anchor_target(value, file, location)
        .map(TypeKey::Anchor)
        .map_err(|_| {
            CatalogError::file(
                file,
                format!("malformed type key {value:?}; expected idl:Name or SPEC#anchor"),
            )
            .at(location)
        })
}

fn compile_type(
    raw: RawTypeDeclaration,
    public_id: String,
    file: &str,
) -> Result<TypeDeclaration, CatalogError> {
    let location = format!("type {public_id}");
    Ok(TypeDeclaration {
        ty: anchor_target(&raw.ty, file, &location)?,
        name: raw.name,
        kind: raw.kind,
        alias_of: raw
            .alias_of
            .as_deref()
            .map(|value| type_key(value, file, &location))
            .transpose()?,
        implemented_by: raw
            .implemented_by
            .iter()
            .map(|value| type_key(value, file, &location))
            .collect::<Result<_, _>>()?,
        expect_text: compile_pattern(&raw.expect_text, true, file, &location)?,
        reason: required_text(raw.reason, "reason", file, &location)?,
        source_file: file.to_owned(),
        public_id,
    })
}

fn compile_field(
    raw: RawFieldDeclaration,
    public_id: String,
    file: &str,
) -> Result<FieldDeclaration, CatalogError> {
    let location = format!("field {public_id}");
    let owner = match raw.owner {
        Some(owners) if owners.is_empty() => {
            return Err(CatalogError::file(file, "owner must not be empty").at(location));
        }
        Some(owners) => Some(
            owners
                .iter()
                .map(|value| type_key(value, file, &location))
                .collect::<Result<_, _>>()?,
        ),
        None => None,
    };
    Ok(FieldDeclaration {
        field: anchor_target(&raw.field, file, &location)?,
        owner,
        ty: raw
            .ty
            .as_ref()
            .map(|value| type_expr(value, false, file, &location))
            .transpose()?,
        initial: raw
            .initial
            .as_ref()
            .map(|value| initial_value(value, file, &location))
            .transpose()?,
        expect_text: compile_pattern(&raw.expect_text, true, file, &location)?,
        reason: required_text(raw.reason, "reason", file, &location)?,
        source_file: file.to_owned(),
        public_id,
    })
}

fn required_text(
    value: String,
    what: &str,
    file: &str,
    location: &str,
) -> Result<String, CatalogError> {
    if value.trim().is_empty() {
        Err(CatalogError::file(file, format!("{what} must not be empty")).at(location))
    } else {
        Ok(value)
    }
}

/// The single `key: value` pair of a one-key mapping.
fn single_entry(value: &Value) -> Option<(&str, &Value)> {
    match value {
        Value::Object(map) if map.len() == 1 => map.iter().next().map(|(k, v)| (k.as_str(), v)),
        _ => None,
    }
}

fn type_expr(
    value: &Value,
    in_union: bool,
    file: &str,
    location: &str,
) -> Result<TypeExpr, CatalogError> {
    let error = |message: String| CatalogError::file(file, message).at(location);
    if let Value::String(word) = value {
        let primitive = match word.as_str() {
            "boolean" => Primitive::Boolean,
            "string" => Primitive::String,
            "number" => Primitive::Number,
            "integer" => Primitive::Integer,
            "byte_sequence" => Primitive::ByteSequence,
            "scalar_value_string" => Primitive::ScalarValueString,
            _ => return Err(error(format!("unknown primitive type {word:?}"))),
        };
        return Ok(TypeExpr::Primitive(primitive));
    }
    if value.is_null() && in_union {
        return Ok(TypeExpr::Null);
    }
    let Some((key, inner)) = single_entry(value) else {
        return Err(error(format!(
            "invalid type {value}; expected a primitive word or a one-key mapping \
             (nominal, list, ordered_set, ordered_map, map, union, opaque)"
        )));
    };
    let infra = |kind: InfraKind| -> Result<TypeExpr, CatalogError> {
        Ok(TypeExpr::Infra {
            kind,
            args: vec![type_expr(inner, false, file, location)?],
        })
    };
    match key {
        "nominal" => {
            let Value::String(text) = inner else {
                return Err(error("nominal type must be a SPEC#anchor string".into()));
            };
            Ok(TypeExpr::Nominal {
                ty: TypeRef::Unresolved(anchor_target(text, file, location)?),
                text: text.clone(),
            })
        }
        "list" => infra(InfraKind::List),
        "ordered_set" => infra(InfraKind::OrderedSet),
        "ordered_map" => infra(InfraKind::OrderedMap),
        "map" => infra(InfraKind::Map),
        "union" => {
            let Value::Array(members) = inner else {
                return Err(error("union must be a sequence of types".into()));
            };
            if members.len() < 2 {
                return Err(error("union needs at least two members".into()));
            }
            members
                .iter()
                .map(|member| type_expr(member, true, file, location))
                .collect::<Result<_, _>>()
                .map(TypeExpr::Union)
        }
        "opaque" => match inner {
            Value::String(text) if !text.trim().is_empty() => {
                Ok(TypeExpr::Opaque { text: text.clone() })
            }
            _ => Err(error("opaque type must be a non-empty string".into())),
        },
        _ => Err(error(format!("unknown type form {key:?}"))),
    }
}

/// `empty` and `unset` are keywords; every other string is a string literal.
fn initial_value(value: &Value, file: &str, location: &str) -> Result<InitialValue, CatalogError> {
    let text = serde_json::to_string(value).unwrap_or_default();
    Ok(match value {
        Value::Bool(flag) => InitialValue::Literal {
            value: Literal::Bool(*flag),
            text,
        },
        Value::Null => InitialValue::Literal {
            value: Literal::Null,
            text,
        },
        Value::Number(number) => InitialValue::Literal {
            value: Literal::Number(number.to_string()),
            text,
        },
        Value::String(word) if word == "empty" => InitialValue::Empty { text: word.clone() },
        Value::String(word) if word == "unset" => InitialValue::Unset { text: word.clone() },
        Value::String(string) => InitialValue::Literal {
            value: Literal::String(string.clone()),
            text,
        },
        _ => match single_entry(value) {
            Some(("opaque", Value::String(text))) if !text.trim().is_empty() => {
                InitialValue::Opaque { text: text.clone() }
            }
            _ => {
                return Err(CatalogError::file(
                    file,
                    format!(
                        "invalid initial value {value}; expected a literal, empty, unset \
                         or {{opaque: text}}"
                    ),
                )
                .at(location))
            }
        },
    })
}

fn compile_rule(
    raw: RawStateRule,
    public_id: String,
    file: &str,
) -> Result<StateRule, CatalogError> {
    let location = format!("rule {public_id}");
    let match_spec = compile_match(&raw.match_spec, file, &location)?;
    let emit = compile_emit(
        &raw.emit,
        match_spec.text.as_ref(),
        &public_id,
        file,
        &location,
    )?;
    Ok(StateRule {
        public_id,
        match_spec,
        emit,
        description: raw.description,
        source_file: file.to_owned(),
    })
}

fn compile_emit(
    raw: &RawStateEmit,
    text: Option<&CatalogPattern>,
    rule: &str,
    file: &str,
    location: &str,
) -> Result<StateEmit, CatalogError> {
    let error =
        |message: String| CatalogError::file(file, format!("rule {rule}: {message}")).at(location);
    let allowed: &[&str] = match raw.kind.as_str() {
        "state.write" => &["field", "op"],
        "state.init" => &["field"],
        "state.mutate" => &["op", "target", "operand"],
        kind => {
            return Err(error(format!(
                "unknown state emit kind {kind:?}; expected state.write, state.init or state.mutate"
            )))
        }
    };
    if let Some(name) = raw
        .params
        .keys()
        .find(|name| !allowed.contains(&name.as_str()))
    {
        return Err(error(format!(
            "unknown parameter {name:?} for {}; expected {}",
            raw.kind,
            allowed.join(", ")
        )));
    }
    let string = |name: &str| -> Result<String, CatalogError> {
        match raw.params.get(name) {
            Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
            Some(_) => Err(error(format!(
                "parameter {name:?} must be a non-empty string"
            ))),
            None => Err(error(format!("{} requires parameter {name:?}", raw.kind))),
        }
    };
    let capture = |name: &str, source: &str| -> Result<Option<String>, CatalogError> {
        let Some(value) = raw.params.get(name) else {
            return Ok(None);
        };
        let fields: BTreeMap<&str, &Value> = match value {
            Value::Object(map) => map.iter().map(|(k, v)| (k.as_str(), v)).collect(),
            _ => BTreeMap::new(),
        };
        let (Some(Value::String(capture)), Some(Value::String(from)), 2) =
            (fields.get("capture"), fields.get("from"), fields.len())
        else {
            return Err(error(format!(
                "parameter {name:?} must be {{capture: NAME, from: {source}}}"
            )));
        };
        if from != source {
            return Err(error(format!(
                "parameter {name:?} captures from {from:?}; expected {source}"
            )));
        }
        let known = text.is_some_and(|pattern| {
            pattern
                .regex()
                .capture_names()
                .flatten()
                .any(|group| group == capture)
        });
        if !known {
            return Err(error(format!(
                "unknown regex capture group {capture:?} in parameter {name:?}"
            )));
        }
        Ok(Some(capture.clone()))
    };
    let field = || -> Result<AnchorTarget, CatalogError> {
        anchor_target(&string("field")?, file, location).map_err(|e| error(e.message))
    };
    Ok(match raw.kind.as_str() {
        "state.write" => StateEmit::Write {
            field: field()?,
            op: string("op")?,
        },
        "state.init" => StateEmit::Init { field: field()? },
        _ => StateEmit::Mutate {
            op: string("op")?,
            target: capture("target", "path")?
                .ok_or_else(|| error("state.mutate requires parameter \"target\"".into()))?,
            operand: capture("operand", "text")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "schema: 1\npackage: p\n";

    fn load(body: &str) -> Result<StateCatalog, CatalogError> {
        load_state_files(&[("state/a.yaml", &format!("{HEADER}{body}"))])
    }

    #[test]
    fn loads_the_spec_examples() {
        let yaml = include_str!("../../tests/fixtures/state/catalog/spec-examples.yaml");
        let c = load_state_files(&[("state/examples.yaml", yaml)]).unwrap();
        assert_eq!(c.types.len(), 3);
        assert_eq!(c.types[1].name.as_deref(), Some("traversal"));
        assert_eq!(c.fields.len(), 2);
        assert!(matches!(c.rules[0].emit, StateEmit::Mutate { ref op, .. } if op == "append"));
        assert!(c.digest.as_deref().unwrap().starts_with("sha256:"));
    }

    #[test]
    fn rejects_unknown_kinds_missing_expect_text_and_unknown_captures() {
        let bad_kind = "schema: 1\npackage: p\nrules:\n  - id: r\n    match: {text: 'x'}\n    emit: {kind: event.fire}\n";
        assert!(load_state_files(&[("state/a.yaml", bad_kind)])
            .unwrap_err()
            .message
            .contains("state.write"));
        let no_expect = "schema: 1\npackage: p\nfields:\n  - id: f\n    field: HTML#f\n    owner: [HTML#o]\n    reason: r\n";
        assert!(load_state_files(&[("state/a.yaml", no_expect)]).is_err());
        let bad_capture = "schema: 1\npackage: p\nrules:\n  - id: r\n    match: {text: 'Add (?P<t>.+)'}\n    emit: {kind: state.mutate, params: {op: append, target: {capture: nope, from: path}}}\n";
        assert!(load_state_files(&[("state/a.yaml", bad_capture)])
            .unwrap_err()
            .message
            .contains("nope"));
    }

    #[test]
    fn spec_examples_compile_to_typed_declarations_and_emits() {
        let yaml = include_str!("../../tests/fixtures/state/catalog/spec-examples.yaml");
        let c = load_state_files(&[("state/examples.yaml", yaml)]).unwrap();
        let types = &c.types;
        assert_eq!(types[0].public_id, "webspec-index/html-media-element-alias");
        assert_eq!(types[0].alias_of, TypeKey::parse("idl:HTMLMediaElement"));
        assert_eq!(types[1].kind, Some(TypeKind::Concept));
        assert_eq!(
            types[1].implemented_by,
            vec![
                TypeKey::Idl("NodeIterator".into()),
                TypeKey::Idl("TreeWalker".into())
            ]
        );
        let option = &c.fields[0];
        assert_eq!(
            option.owner,
            Some(vec![TypeKey::parse("HTML#the-option-element").unwrap()])
        );
        assert_eq!(option.ty, Some(TypeExpr::Primitive(Primitive::Boolean)));
        assert_eq!(
            option.initial,
            Some(InitialValue::Literal {
                value: Literal::Bool(false),
                text: "false".into()
            })
        );
        assert_eq!(
            c.fields[1].ty,
            Some(TypeExpr::Opaque {
                text: "a date and time".into()
            })
        );
        assert_eq!(
            c.rules[0].emit,
            StateEmit::Mutate {
                op: "append".into(),
                target: "target".into(),
                operand: Some("operand".into())
            }
        );
        assert_eq!(
            c.rules[2].emit,
            StateEmit::Write {
                field: AnchorTarget {
                    spec: "HTML".into(),
                    anchor: "current-transformation-matrix".into()
                },
                op: "set".into()
            }
        );
        assert!(c.has_declarations() && !c.is_empty());
    }

    #[test]
    fn field_types_and_initial_values_cover_every_yaml_form() {
        let c = load(
            "fields:\n  - id: f\n    field: HTML#f\n    type: {union: [{nominal: HTML#navigable}, {list: {ordered_map: string}}, null]}\n    initial: 'unset'\n    expect_text: x\n    reason: r\n  - id: g\n    field: HTML#g\n    initial: null\n    expect_text: x\n    reason: r\n  - id: h\n    field: HTML#h\n    initial: {opaque: a new list}\n    expect_text: x\n    reason: r\n  - id: i\n    field: HTML#i\n    initial: empty\n    expect_text: x\n    reason: r\n  - id: j\n    field: HTML#j\n    initial: 'label'\n    expect_text: x\n    reason: r\n",
        )
        .unwrap();
        let navigable = AnchorTarget {
            spec: "HTML".into(),
            anchor: "navigable".into(),
        };
        assert_eq!(
            c.fields[0].ty,
            Some(TypeExpr::Union(vec![
                TypeExpr::Nominal {
                    ty: TypeRef::Unresolved(navigable),
                    text: "HTML#navigable".into()
                },
                TypeExpr::Infra {
                    kind: InfraKind::List,
                    args: vec![TypeExpr::Infra {
                        kind: InfraKind::OrderedMap,
                        args: vec![TypeExpr::Primitive(Primitive::String)]
                    }]
                },
                TypeExpr::Null,
            ]))
        );
        assert!(matches!(
            c.fields[0].initial,
            Some(InitialValue::Unset { .. })
        ));
        assert!(matches!(
            c.fields[1].initial,
            Some(InitialValue::Literal {
                value: Literal::Null,
                ..
            })
        ));
        assert!(
            matches!(&c.fields[2].initial, Some(InitialValue::Opaque { text }) if text == "a new list")
        );
        assert!(matches!(
            c.fields[3].initial,
            Some(InitialValue::Empty { .. })
        ));
        assert!(matches!(
            &c.fields[4].initial,
            Some(InitialValue::Literal { value: Literal::String(s), .. }) if s == "label"
        ));
        assert_eq!(c.fields[1].ty, None);
    }

    #[test]
    fn rejects_malformed_declarations() {
        let field = |extra: &str| {
            load(&format!(
                "fields:\n  - id: f\n    field: HTML#f\n{extra}    expect_text: x\n    reason: r\n"
            ))
        };
        assert!(field("    owner: [Document]\n").is_err());
        assert!(field("    owner: []\n").is_err());
        assert!(field("    type: bool\n").is_err());
        assert!(field("    type: null\n").is_err());
        assert!(field("    type: {list: string, map: string}\n").is_err());
        assert!(field("    initial: [1]\n").is_err());
        assert!(field("    extra: 1\n").is_err());
        assert!(load("types:\n  - id: t\n    type: HTML#t\n    expect_text: x\n").is_err());
        assert!(load("types:\n  - id: t\n    type: HTML#t\n    alias_of: Foo\n    expect_text: x\n    reason: r\n").is_err());
        assert!(
            load("types:\n  - id: T\n    type: HTML#t\n    expect_text: x\n    reason: r\n")
                .is_err()
        );
        let dup = "types:\n  - id: t\n    type: HTML#t\n    expect_text: x\n    reason: r\nfields:\n  - id: t\n    field: HTML#f\n    expect_text: x\n    reason: r\n";
        assert!(load(dup).unwrap_err().message.contains("duplicate"));
    }

    #[test]
    fn rejects_malformed_emits_naming_the_rule() {
        let rule = |emit: &str| {
            load(&format!(
                "rules:\n  - id: r\n    match: {{text: 'Add (?P<t>.+)'}}\n    emit: {emit}\n"
            ))
            .unwrap_err()
            .message
        };
        for message in [
            rule("{kind: state.write, params: {field: HTML#f}}"),
            rule("{kind: state.write, params: {field: HTML#f, op: set, extra: 1}}"),
            rule("{kind: state.init, params: {field: nope}}"),
            rule("{kind: state.mutate, params: {op: append}}"),
            rule("{kind: state.mutate, params: {op: append, target: {capture: t, from: text}}}"),
            rule("{kind: state.mutate, params: {op: append, target: {capture: t, from: path}, operand: {capture: t, from: path}}}"),
        ] {
            assert!(message.contains("rule p/r"), "{message}");
        }
    }

    #[test]
    fn only_state_files_are_read_and_digested() {
        let effects = ("rules.yaml", "schema: 1\npackage: p\neffects: {}\n");
        let empty = load_state_files(&[effects]).unwrap();
        assert!(empty.is_empty() && empty.digest.is_none());
        assert_eq!(empty.representation_version(), STATE_VERSION);
        let state = ("state/a.yaml", "schema: 1\npackage: p\n");
        let a = load_state_files(&[effects, state]).unwrap();
        let b = load_state_files(&[state]).unwrap();
        assert_eq!(a.digest, b.digest);
        let expected = format!(
            "sha256:{}",
            sha256_bytes(b"state/a.yaml\0schema: 1\npackage: p\n\0")
        );
        assert_eq!(a.digest.as_deref(), Some(expected.as_str()));
        assert_eq!(
            a.representation_version(),
            format!("{STATE_VERSION}+{expected}")
        );
        let other = load_state_files(&[("state/b.yaml", "schema: 1\npackage: q\n"), state]);
        assert!(other.unwrap_err().message.contains("package is"));
    }

    #[test]
    fn merge_rejects_duplicate_public_ids_and_combines_digests() {
        let rule = "rules:\n  - id: r\n    match: {text: 'x'}\n    emit: {kind: state.init, params: {field: HTML#f}}\n";
        let a = load(rule).unwrap();
        assert!(merge([a.clone(), a.clone()]).is_err());
        let b = load_state_files(&[("state/b.yaml", &format!("schema: 1\npackage: q\n{rule}"))])
            .unwrap();
        let ab = merge([a.clone(), b.clone()]).unwrap();
        let ba = merge([b, a.clone()]).unwrap();
        assert_eq!(ab.rules.len(), 2);
        assert_eq!(ab.digest, ba.digest);
        assert_ne!(ab.digest, a.digest);
        assert_eq!(
            merge([a.clone(), StateCatalog::default()]).unwrap().digest,
            a.digest
        );
        assert!(merge([]).unwrap().digest.is_none());
    }

    #[cfg(feature = "native")]
    #[test]
    fn package_directory_reads_only_its_state_subtree() {
        let dir = std::env::temp_dir().join(format!("state-catalog-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("state/nested")).unwrap();
        std::fs::write(dir.join("root.yaml"), "not: [state").unwrap();
        std::fs::write(dir.join("state/nested/a.yaml"), "schema: 1\npackage: p\n").unwrap();
        let from_dir = load_state_package(&dir).unwrap();
        let from_files =
            load_state_files(&[("state/nested/a.yaml", "schema: 1\npackage: p\n")]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(from_dir.digest, from_files.digest);
    }
}
