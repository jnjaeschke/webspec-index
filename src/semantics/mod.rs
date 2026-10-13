//! Rule primitives shared by the effects and state catalogs: YAML loading,
//! anchors, patterns, match specifications and capture extraction.

use crate::parse::steps::{InlineToken, InlineTokenKind, LinkSpan, StructuralSegment};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
#[cfg(feature = "native")]
use std::path::{Path, PathBuf};
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;
use yaml_rust2::{Yaml, YamlLoader};

pub(crate) mod bundled;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError {
    pub file: Option<String>,
    pub location: Option<String>,
    pub message: String,
}

impl CatalogError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            file: None,
            location: None,
            message: message.into(),
        }
    }

    pub(crate) fn file(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            file: Some(file.into()),
            location: None,
            message: message.into(),
        }
    }

    pub(crate) fn at(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}")?;
            if let Some(location) = &self.location {
                write!(f, " ({location})")?;
            }
            write!(f, ": ")?;
        }
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CatalogError {}

#[derive(Debug, Clone)]
pub struct CatalogPattern {
    source: String,
    regex: Regex,
}

impl CatalogPattern {
    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn regex(&self) -> &Regex {
        &self.regex
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Anchor {
    pub spec: String,
    pub anchor: String,
}

impl Anchor {
    pub fn as_identity(&self) -> String {
        format!("{}#{}", self.spec, self.anchor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    Literal,
    Text,
    Anchor,
}

#[derive(Debug, Clone)]
pub struct MatchSpec {
    pub anchor: Option<Anchor>,
    pub subject: Option<Anchor>,
    pub text: Option<CatalogPattern>,
    pub exclude_text: Vec<CatalogPattern>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMatchSpec {
    pub anchor: Option<String>,
    pub subject: Option<String>,
    pub text: Option<String>,
    #[serde(default)]
    pub exclude_text: Vec<String>,
}

pub fn compile_match(
    raw: &RawMatchSpec,
    file: &str,
    location: &str,
) -> Result<MatchSpec, CatalogError> {
    let anchor = raw
        .anchor
        .as_deref()
        .map(|value| parse_anchor(value, file, location))
        .transpose()?;
    let subject = raw
        .subject
        .as_deref()
        .map(|value| parse_anchor(value, file, location))
        .transpose()?;
    let text = raw
        .text
        .as_deref()
        .map(|value| compile_pattern(value, false, file, location))
        .transpose()?;
    if anchor.is_none() && text.is_none() {
        return Err(
            CatalogError::file(file, "match requires at least anchor or text").at(location),
        );
    }
    let exclude_text = raw
        .exclude_text
        .iter()
        .map(|value| compile_pattern(value, true, file, location))
        .collect::<Result<_, _>>()?;
    Ok(MatchSpec {
        anchor,
        subject,
        text,
        exclude_text,
    })
}

/// Parse one catalog YAML file into `T`. Rejects multiple documents, aliases,
/// anchors, merge keys, custom tags and floats before deserializing.
pub fn parse_yaml_file<T: DeserializeOwned>(path: &str, content: &str) -> Result<T, CatalogError> {
    let mut preflight = YamlPreflight::default();
    Parser::new_from_str(content)
        .load(&mut preflight, true)
        .map_err(|error| CatalogError::file(path, format!("invalid YAML: {error}")))?;
    if let Some(error) = preflight.error {
        return Err(CatalogError::file(path, error));
    }
    if preflight.documents != 1 {
        return Err(CatalogError::file(
            path,
            format!(
                "expected exactly one YAML document, found {}",
                preflight.documents
            ),
        ));
    }
    let mut docs = YamlLoader::load_from_str(content)
        .map_err(|error| CatalogError::file(path, format!("invalid YAML: {error}")))?;
    if docs.len() != 1 {
        return Err(CatalogError::file(
            path,
            "expected exactly one YAML document",
        ));
    }
    let value = yaml_to_json(docs.remove(0), path, "$")?;
    let encoded = serde_json::to_string(&value)
        .map_err(|error| CatalogError::file(path, format!("cannot decode catalog: {error}")))?;
    let mut deserializer = serde_json::Deserializer::from_str(&encoded);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        CatalogError::file(path, format!("invalid catalog fields: {}", error.inner()))
            .at(error.path().to_string())
    })
}

#[derive(Default)]
struct YamlPreflight {
    documents: usize,
    error: Option<String>,
}

impl MarkedEventReceiver for YamlPreflight {
    fn on_event(&mut self, event: Event, marker: Marker) {
        if self.error.is_some() {
            return;
        }
        let at = || format!("line {}, column {}", marker.line() + 1, marker.col() + 1);
        match event {
            Event::DocumentStart => self.documents += 1,
            Event::Alias(_) => {
                self.error = Some(format!("YAML aliases are not supported ({})", at()))
            }
            Event::Scalar(_, _, anchor, tag) => self.check_node(anchor, tag, &at()),
            Event::SequenceStart(anchor, tag) | Event::MappingStart(anchor, tag) => {
                self.check_node(anchor, tag, &at())
            }
            _ => {}
        }
    }
}

impl YamlPreflight {
    fn check_node(&mut self, anchor: usize, tag: Option<yaml_rust2::parser::Tag>, at: &str) {
        if anchor != 0 {
            self.error = Some(format!("YAML anchors are not supported ({at})"));
        } else if let Some(tag) = tag {
            let standard = tag.handle == "tag:yaml.org,2002:"
                && matches!(tag.suffix.as_str(), "str" | "bool" | "int" | "null");
            if !standard {
                self.error = Some(format!("custom YAML tags are not supported ({at})"));
            }
        }
    }
}

fn yaml_to_json(yaml: Yaml, file: &str, location: &str) -> Result<Value, CatalogError> {
    match yaml {
        Yaml::Null => Ok(Value::Null),
        Yaml::Boolean(value) => Ok(Value::Bool(value)),
        Yaml::Integer(value) => Ok(Value::Number(value.into())),
        Yaml::String(value) => Ok(Value::String(value)),
        Yaml::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| yaml_to_json(value, file, &format!("{location}[{index}]")))
            .collect(),
        Yaml::Hash(values) => {
            let mut output = serde_json::Map::new();
            for (key, value) in values {
                let Yaml::String(key) = key else {
                    return Err(
                        CatalogError::file(file, "mapping keys must be strings").at(location)
                    );
                };
                if key == "<<" {
                    return Err(
                        CatalogError::file(file, "YAML merge keys are not supported").at(location),
                    );
                }
                output.insert(
                    key.clone(),
                    yaml_to_json(value, file, &format!("{location}.{key}"))?,
                );
            }
            Ok(Value::Object(output))
        }
        Yaml::Real(_) => {
            Err(CatalogError::file(file, "floating-point values are not supported").at(location))
        }
        Yaml::Alias(_) => {
            Err(CatalogError::file(file, "YAML aliases are not supported").at(location))
        }
        Yaml::BadValue => Err(CatalogError::file(file, "invalid YAML value").at(location)),
    }
}

/// Checks `value` against `^first rest*$` without compiling a regex: the
/// catalog validates every ID in every process that loads it.
pub(crate) fn matches_identifier(
    value: &str,
    first: fn(char) -> bool,
    rest: fn(char) -> bool,
) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(first) && chars.all(rest)
}

pub fn validate_id(value: &str, what: &str, file: &str) -> Result<(), CatalogError> {
    if matches_identifier(
        value,
        |c| c.is_ascii_lowercase() || c.is_ascii_digit(),
        |c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'),
    ) {
        Ok(())
    } else {
        Err(CatalogError::file(
            file,
            format!("invalid {what} {value:?}; expected [a-z0-9][a-z0-9._-]*"),
        ))
    }
}

pub fn parse_anchor(value: &str, file: &str, location: &str) -> Result<Anchor, CatalogError> {
    if value.chars().any(char::is_whitespace) || value.matches('#').count() != 1 {
        return Err(CatalogError::file(
            file,
            format!("malformed canonical anchor {value:?}; expected SPEC#anchor"),
        )
        .at(location));
    }
    let (spec, anchor) = value.split_once('#').expect("one delimiter checked");
    if spec.is_empty() || anchor.is_empty() {
        return Err(CatalogError::file(
            file,
            format!("malformed canonical anchor {value:?}; expected SPEC#anchor"),
        )
        .at(location));
    }
    let registry = crate::spec_registry::SpecRegistry::new();
    let canonical_spec = registry
        .infer_base_url_from_spec_name(spec)
        .and_then(|(base, _)| {
            registry.resolve_url(&format!("{}#{anchor}", base.trim_end_matches('#')))
        })
        .map(|(name, _)| name)
        .unwrap_or_else(|| spec.to_owned());
    Ok(Anchor {
        spec: canonical_spec,
        anchor: anchor.to_owned(),
    })
}

/// Compile a catalog regex. Match patterns (`allow_empty == false`) must not be
/// able to match the empty string; searched patterns (exclusions, expected
/// text) may.
pub fn compile_pattern(
    value: &str,
    allow_empty: bool,
    file: &str,
    location: &str,
) -> Result<CatalogPattern, CatalogError> {
    let regex = Regex::new(value).map_err(|error| {
        CatalogError::file(file, format!("invalid Rust regex {value:?}: {error}")).at(location)
    })?;
    if !allow_empty && regex.is_match("") {
        return Err(CatalogError::file(
            file,
            format!("match regex {value:?} is capable of an empty match"),
        )
        .at(location));
    }
    Ok(CatalogPattern {
        source: value.to_owned(),
        regex,
    })
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::hex::encode(&Sha256::digest(bytes))
}

/// Append every `.yaml`/`.yml` file under `directory` as `(path relative to
/// root with '/' separators, absolute path)`. Symlinks are skipped.
#[cfg(feature = "native")]
pub fn collect_yaml_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<(String, PathBuf)>,
) -> Result<(), CatalogError> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        CatalogError::new(format!(
            "cannot read catalog directory {}: {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CatalogError::new(format!("cannot read catalog entry: {error}")))?;
        let file_type = entry
            .file_type()
            .map_err(|error| CatalogError::new(error.to_string()))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_yaml_files(root, &path, output)?;
        } else if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension == "yaml" || extension == "yml")
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| CatalogError::new(error.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            output.push((relative, path));
        }
    }
    Ok(())
}

/// Canonical inline text with the tokens and links whose spans index into it.
pub trait TextView {
    fn text(&self) -> &str;
    fn tokens(&self) -> &[InlineToken];
    fn links(&self) -> &[LinkSpan];
}

impl TextView for StructuralSegment {
    fn text(&self) -> &str {
        &self.text
    }

    fn tokens(&self) -> &[InlineToken] {
        &self.tokens
    }

    fn links(&self) -> &[LinkSpan] {
        &self.links
    }
}

/// The value of a capture covering `start..end` of `view.text()`. Literal and
/// anchor sources require the capture to cover a complete token or link.
pub fn extract_capture(
    view: &dyn TextView,
    start: usize,
    end: usize,
    source: CaptureSource,
) -> Option<String> {
    let text = view.text();
    match source {
        CaptureSource::Text => text.get(start..end).map(str::to_owned),
        CaptureSource::Literal => {
            let token = view.tokens().iter().find(|token| {
                token.span.start == start
                    && token.span.end == end
                    && token.kind == InlineTokenKind::Literal
            })?;
            let raw = text.get(start..end)?;
            Some(raw.trim_matches(['`', '"', '\'']).to_owned())
                .filter(|v| !v.is_empty())
                .or_else(|| Some(token.source_text.clone()))
        }
        CaptureSource::Anchor => view
            .links()
            .iter()
            .find(|link| link.span.start == start && link.span.end == end)
            .and_then(|link| link.target.as_ref())
            .map(|target| format!("{}#{}", target.spec, target.anchor)),
    }
}

/// Anchorless, subject-scoped text matches grouped by whole-match span, as effects' match_segment does.
///
/// Within a span, matches keep rule order, then occurrence order. A rule is
/// skipped when its subject differs from `subject` or any exclusion matches
/// anywhere in `text`.
pub fn scoped_text_matches<'r, 't, R>(
    rules: impl IntoIterator<Item = (&'r R, &'r MatchSpec)>,
    subject: &Anchor,
    text: &'t str,
) -> BTreeMap<(usize, usize), Vec<(&'r R, regex::Captures<'t>)>>
where
    R: ?Sized + 'r,
{
    let mut by_span: BTreeMap<(usize, usize), Vec<(&'r R, regex::Captures<'t>)>> = BTreeMap::new();
    for (rule, spec) in rules {
        if spec.anchor.is_some()
            || spec
                .subject
                .as_ref()
                .is_some_and(|expected| expected != subject)
            || spec
                .exclude_text
                .iter()
                .any(|pattern| pattern.regex().is_match(text))
        {
            continue;
        }
        let Some(pattern) = &spec.text else {
            continue;
        };
        for captures in pattern.regex().captures_iter(text) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            by_span
                .entry((whole.start(), whole.end()))
                .or_default()
                .push((rule, captures));
        }
    }
    by_span
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(feature = "native", unix))]
    #[test]
    fn collect_yaml_files_walks_nested_yaml_and_skips_symlinks_and_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("state/nested")).unwrap();
        std::fs::write(root.join("a.yaml"), "").unwrap();
        std::fs::write(root.join("state/b.yml"), "").unwrap();
        std::fs::write(root.join("state/nested/c.yaml"), "").unwrap();
        std::fs::write(root.join("state/notes.txt"), "").unwrap();
        std::os::unix::fs::symlink(root.join("a.yaml"), root.join("state/link.yaml")).unwrap();
        std::os::unix::fs::symlink(root.join("state/nested"), root.join("linked-dir")).unwrap();

        let mut output = Vec::new();
        collect_yaml_files(root, &root.join("state"), &mut output).unwrap();
        let mut relative: Vec<_> = output.iter().map(|(path, _)| path.as_str()).collect();
        relative.sort();
        assert_eq!(relative, ["state/b.yml", "state/nested/c.yaml"]);
        assert!(output
            .iter()
            .all(|(path, absolute)| *absolute == root.join(path)));

        output.clear();
        collect_yaml_files(root, root, &mut output).unwrap();
        let mut relative: Vec<_> = output.iter().map(|(path, _)| path.as_str()).collect();
        relative.sort();
        assert_eq!(relative, ["a.yaml", "state/b.yml", "state/nested/c.yaml"]);
    }

    #[test]
    fn scoped_matches_respect_subject_and_exclusions() {
        let raw = |text: &str, subject: Option<&str>, exclude: &[&str]| RawMatchSpec {
            anchor: None,
            subject: subject.map(str::to_string),
            text: Some(text.to_string()),
            exclude_text: exclude.iter().map(|s| s.to_string()).collect(),
        };
        let a = compile_match(&raw("Add (?P<x>\\S+)", None, &[]), "f", "l").unwrap();
        let b = compile_match(&raw("Add", Some("T#other"), &[]), "f", "l").unwrap();
        let c = compile_match(&raw("Add", None, &["local"]), "f", "l").unwrap();
        let rules = [("a", &a), ("b", &b), ("c", &c)];
        let subject = Anchor {
            spec: "T".into(),
            anchor: "s".into(),
        };
        let found = scoped_text_matches(
            rules.iter().map(|(n, m)| (n, *m)),
            &subject,
            "Add x to local.",
        );
        let names: Vec<_> = found.values().flatten().map(|(n, _)| **n).collect();
        assert_eq!(names, ["a"]);
    }
}
