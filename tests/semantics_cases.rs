//! Acceptance cases for the bundled semantics catalog: each case parses its
//! spec excerpts, analyzes one subject, and compares the effects, their
//! witnesses, and the issues reported. No network or database access.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use webspec_index::effects::{
    analyze, default_catalog, AnalysisInput, AnalysisScope, Catalog, DiscoveryBudgets, Execution,
    ExplanationOptions, IndexedAnchor, SourceSpec, SubjectSelector,
};
use webspec_index::parse::steps::{extract_step_structure, BodyKind};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    sources: Vec<CaseSource>,
    subject: SubjectSelector,
    #[serde(default)]
    expect_local_effects: Vec<ExpectedEffect>,
    #[serde(default)]
    expect_effects: Vec<ExpectedEffect>,
    #[serde(default)]
    forbidden_effects: Vec<ForbiddenEffect>,
    #[serde(default)]
    expect_issues: Vec<String>,
    #[serde(default)]
    metadata: CaseMetadata,
}

#[derive(Default, Deserialize)]
struct CaseMetadata {
    environment: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseSource {
    spec: String,
    file: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedEffect {
    kind: String,
    #[serde(default)]
    params: Map<String, Value>,
    #[serde(default)]
    execution: Vec<String>,
    operation_count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForbiddenEffect {
    kind: String,
    #[serde(default)]
    params: Map<String, Value>,
}

#[test]
fn semantics_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/semantics");
    let catalog = default_catalog(&[]).expect("bundled catalog loads");
    let mut failures = Vec::new();
    let mut count = 0;
    for path in sorted_files(&root.join("cases"), "json").unwrap() {
        count += 1;
        let result = fs::read(&path)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| Ok(serde_json::from_slice::<Case>(&bytes)?))
            .and_then(|case| {
                if case.id != path.file_stem().unwrap().to_string_lossy() {
                    bail!("id does not match filename");
                }
                validate_case(&root, &case, &catalog)
            });
        if let Err(error) = result {
            failures.push(format!("{}: {error:#}", path.display()));
        }
    }
    assert!(count > 0, "no cases found");
    assert!(
        failures.is_empty(),
        "{} of {count} cases failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

fn validate_case(root: &Path, case: &Case, catalog: &Catalog) -> Result<()> {
    let mut html_by_spec = BTreeMap::<String, String>::new();
    for source in &case.sources {
        let path = root.join(&source.file);
        let html = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        html_by_spec
            .entry(source.spec.clone())
            .or_default()
            .push_str(&html);
    }
    let mut sources = Vec::new();
    for (spec, html) in html_by_spec {
        let sha = format!("{:x}", Sha256::digest(html.as_bytes()));
        let base = base_url(&spec);
        let structure = extract_step_structure(&html, &spec, &base, &sha);
        if structure.algorithms.is_empty() {
            bail!("{} sources parsed no algorithms", spec);
        }
        let anchors = structure
            .algorithms
            .iter()
            .map(|algorithm| IndexedAnchor {
                anchor: algorithm.source.section_anchor.clone(),
                url: algorithm.source.url.clone(),
                text: algorithm
                    .segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                idl_kind: None,
            })
            .collect();
        sources.push(SourceSpec {
            spec,
            snapshot_sha: sha,
            base_url: base,
            structure: Some(structure),
            anchors,
        });
    }
    add_target_stubs(&mut sources);
    let named_body_ids = sources
        .iter()
        .filter_map(|source| source.structure.as_ref())
        .flat_map(|structure| structure.algorithms.iter())
        .flat_map(|algorithm| algorithm.bodies.iter())
        .filter(|body| body.kind == BodyKind::Named)
        .map(|body| body.source.node_id.clone())
        .collect::<BTreeSet<_>>();
    let artifact = analyze(AnalysisInput {
        sources: &sources,
        catalog,
        environment: case.metadata.environment.as_deref().unwrap_or("generic"),
        scope: AnalysisScope::Subject {
            subject: case.subject.clone(),
        },
        budgets: DiscoveryBudgets::default(),
    })?;
    let summary = artifact.summary(&case.subject, None, &artifact.sites)?;
    // These are product-level checks: the condensed explanation must retain a
    // connected route and its scheduling boundary, not just the right group name.
    let explained = artifact.explain(
        &case.subject,
        None,
        &ExplanationOptions::default(),
        &artifact.sites,
    )?;
    anyhow::ensure!(
        explained.summary.effects == summary.effects,
        "explaining changed detected effects"
    );
    anyhow::ensure!(
        explained.explanations.len() == summary.effects.len(),
        "missing explanation group"
    );
    for effect in &summary.effects {
        let explanation = explained
            .explanations
            .iter()
            .find(|item| item.effect_id == effect.id)
            .context("effect has no explanation entry")?;
        anyhow::ensure!(
            !explanation.witnesses.is_empty() || explanation.witnesses_truncated,
            "effect has neither a witness nor explicit budget truncation"
        );
        for witness in &explanation.witnesses {
            anyhow::ensure!(
                !witness.terminal_evidence.is_empty(),
                "witness has no origin evidence"
            );
            if let Some(first) = witness.hops.first() {
                anyhow::ensure!(
                    first.from == summary.subject,
                    "witness starts at another subject"
                );
            }
            for pair in witness.hops.windows(2) {
                anyhow::ensure!(pair[0].to == pair[1].from, "witness route is disconnected");
            }
            if effect.execution == [Execution::Separate] {
                anyhow::ensure!(
                    witness.hops.iter().any(|hop| hop
                        .boundary
                        .as_ref()
                        .is_some_and(|boundary| boundary.execution == Execution::Separate)),
                    "separately executed effect lost its scheduling boundary"
                );
            }
        }
    }
    let mut actual_effects = summary
        .effects
        .iter()
        .map(|effect| {
            let operation_count = {
                let in_subject = artifact
                    .occurrences
                    .iter()
                    .filter(|occurrence| {
                        occurrence.kind == effect.kind
                            && occurrence.params == effect.params
                            && occurrence.evidence.iter().any(|e| {
                                e.site.subject.spec == case.subject.spec
                                    && e.site.subject.anchor == case.subject.anchor
                            })
                    })
                    .count();
                (if in_subject > 0 {
                    in_subject
                } else {
                    artifact
                        .occurrences
                        .iter()
                        .filter(|occurrence| {
                            occurrence.kind == effect.kind
                                && occurrence.params == effect.params
                                && !occurrence.subject_id.starts_with("anchor:")
                        })
                        .count()
                }) as u64
            };
            serde_json::json!({
                "kind": effect.kind,
                "params": effect.params,
                "execution": effect.execution,
                "operation_count": operation_count
            })
        })
        .collect::<Vec<_>>();
    let mut expected_effects = case
        .expect_effects
        .iter()
        .map(|effect| {
            serde_json::json!({
                "kind": effect.kind, "params": effect.params, "execution": effect.execution,
                "operation_count": effect.operation_count
            })
        })
        .collect::<Vec<_>>();
    actual_effects.sort_by_key(Value::to_string);
    expected_effects.sort_by_key(Value::to_string);
    if actual_effects != expected_effects {
        let paths = artifact
            .nodes
            .iter()
            .filter(|node| node.subject.anchor == case.subject.anchor)
            .filter_map(|node| node.subject.step_path.clone())
            .collect::<BTreeSet<_>>();
        bail!("effects mismatch\nexpected: {expected_effects:#?}\nactual: {actual_effects:#?}\nstep paths: {paths:?}");
    }
    for forbidden in &case.forbidden_effects {
        if summary.effects.iter().any(|effect| {
            effect.kind == forbidden.kind
                && serde_json::to_value(&effect.params)
                    .is_ok_and(|params| params.as_object() == Some(&forbidden.params))
        }) {
            bail!(
                "forbidden effect was present: {} {:?}",
                forbidden.kind,
                forbidden.params
            );
        }
    }

    let actual_local = artifact
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.evidence.iter().any(|e| {
                e.site.subject.spec == case.subject.spec
                    && e.site.subject.anchor == case.subject.anchor
                    && if let Some(path) = case.subject.step_path.as_ref() {
                        e.site.subject.step_path.as_ref() == Some(path)
                    } else if let Some(body_id) = case.subject.body_id.as_deref() {
                        e.site.subject.body_id.as_deref() == Some(body_id)
                    } else {
                        e.site
                            .subject
                            .body_id
                            .as_ref()
                            .is_none_or(|id| !named_body_ids.contains(id))
                    }
            })
        })
        .fold(
            BTreeMap::<(String, String), u64>::new(),
            |mut counts, occurrence| {
                let params = serde_json::to_string(&occurrence.params).unwrap();
                *counts.entry((occurrence.kind.clone(), params)).or_default() += 1;
                counts
            },
        );
    let expected_local = case
        .expect_local_effects
        .iter()
        .map(|effect| {
            let params = serde_json::to_string(&effect.params).unwrap();
            ((effect.kind.clone(), params), effect.operation_count)
        })
        .collect::<BTreeMap<_, _>>();
    if actual_local != expected_local {
        bail!("local effects mismatch\nexpected: {expected_local:#?}\nactual: {actual_local:#?}");
    }
    let mut actual_issues = summary
        .issues
        .iter()
        .map(|issue| {
            serde_json::to_value(issue.code)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    actual_issues.extend(
        artifact
            .occurrences
            .iter()
            .filter(|occurrence| {
                occurrence.evidence.iter().any(|e| {
                    e.site.subject.spec == case.subject.spec
                        && e.site.subject.anchor == case.subject.anchor
                })
            })
            .flat_map(|occurrence| occurrence.issue_codes.iter())
            .map(|code| {
                serde_json::to_value(code)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            }),
    );
    let expected_issues = case.expect_issues.iter().cloned().collect::<BTreeSet<_>>();
    if actual_issues != expected_issues {
        bail!("issues mismatch: expected {expected_issues:?}, actual {actual_issues:?}");
    }
    Ok(())
}

fn sorted_files(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn base_url(spec: &str) -> String {
    match spec {
        "HTML" => "https://html.spec.whatwg.org/multipage/".into(),
        "DOM" => "https://dom.spec.whatwg.org/".into(),
        "WEBIDL" => "https://webidl.spec.whatwg.org/".into(),
        "ECMA-262" => "https://tc39.es/ecma262/multipage/".into(),
        _ => "https://example.test/spec".into(),
    }
}

fn add_target_stubs(sources: &mut Vec<SourceSpec>) {
    let stubs = [
        ("TEST", &["unknown-operation", "long-target"][..]),
        ("DOM", &["concept-event-fire", "concept-event-dispatch"][..]),
        (
            "HTML",
            &[
                "queue-a-task",
                "queue-a-global-task",
                "queue-a-media-element-task",
                "enqueue-the-following-steps",
                "tn-append-session-history-sync-nav-steps",
                "port-message-queue",
                "queue-an-element-task",
                "queue-a-microtask",
                "tn-append-session-history-traversal-steps",
                "in-parallel",
                "run-a-classic-script",
                "run-a-module-script",
                "prepare-to-run-script",
                "clean-up-after-running-script",
                "prepare-to-run-a-callback",
                "clean-up-after-running-a-callback",
            ][..],
        ),
        (
            "WEBIDL",
            &[
                "invoke-a-callback-function",
                "call-a-user-objects-operation",
            ][..],
        ),
        ("ECMA-262", &["sec-hostenqueuepromisejob"][..]),
    ];
    for (spec, anchors) in stubs {
        if let Some(source) = sources.iter_mut().find(|source| source.spec == spec) {
            for anchor in anchors {
                if !source.anchors.iter().any(|a| a.anchor == *anchor) {
                    source.anchors.push(stub(spec, anchor));
                }
            }
        } else {
            sources.push(SourceSpec {
                spec: spec.into(),
                snapshot_sha: "0".repeat(64),
                base_url: base_url(spec),
                structure: None,
                anchors: anchors.iter().map(|a| stub(spec, a)).collect(),
            });
        }
    }
}

fn stub(spec: &str, anchor: &str) -> IndexedAnchor {
    IndexedAnchor {
        anchor: anchor.into(),
        url: format!("{}#{anchor}", base_url(spec)),
        text: anchor.into(),
        idl_kind: None,
    }
}
