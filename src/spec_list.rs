use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

const CSSWG_URL: &str = "https://github.com/w3c/csswg-drafts";
const GROUPS_URL: &str = "https://github.com/w3c/groups";
const BUNDLED_SPEC_LIST: &str = include_str!("../data/w3c_specs.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecEntry {
    pub name: String,
    pub base_url: String,
    pub provider: String,
    pub github_repo: String,
}

/// Seed the DB from the bundled W3C spec list.
pub fn fetch_and_seed(conn: &Connection) -> Result<usize> {
    let entries: Vec<SpecEntry> = serde_json::from_str(BUNDLED_SPEC_LIST)
        .context("Failed to parse bundled w3c_specs.json")?;
    let count = entries.len();
    for e in &entries {
        crate::db::write::seed_spec(conn, &e.name, &e.base_url, &e.provider)?;
    }
    Ok(count)
}

/// Update the W3C spec list from csswg-drafts and w3c/groups.
///
/// This covers W3C specs only. WHATWG specs (HTML, DOM, Fetch, …) and TC39
/// specs (ECMAScript, …) are small, stable lists hardcoded in their respective
/// providers (`src/provider/whatwg.rs`, `src/provider/tc39.rs`).
pub fn update(
    csswg_dir: &Path,
    groups_dir: &Path,
    output: &Path,
) -> Result<(usize, usize, Vec<SpecEntry>)> {
    clone_or_update(CSSWG_URL, csswg_dir)?;
    clone_or_update(GROUPS_URL, groups_dir)?;

    let csswg = collect_csswg(csswg_dir);
    let standalone = collect_standalone(groups_dir)?;
    let csswg_count = csswg.len();
    let standalone_count = standalone.len();

    let mut all = csswg;
    all.extend(standalone);
    resolve_collisions(&mut all);

    let mut seen_names = std::collections::HashSet::new();
    let mut seen_urls = std::collections::HashSet::new();
    all.retain(|e| seen_names.insert(e.name.clone()) && seen_urls.insert(e.base_url.clone()));
    all.sort_by(|a, b| a.name.cmp(&b.name));

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&all)?;
    std::fs::write(output, format!("{}\n", json))
        .with_context(|| format!("Failed to write {}", output.display()))?;

    Ok((csswg_count, standalone_count, all))
}

fn clone_or_update(url: &str, local_path: &Path) -> Result<()> {
    if local_path.join(".git").is_dir() {
        eprintln!("Updating {} ...", local_path.display());
        let status = Command::new("git")
            .args(["-C", local_path.to_str().unwrap(), "pull", "--depth=1"])
            .status()
            .with_context(|| format!("Failed to run git pull in {}", local_path.display()))?;
        if !status.success() {
            anyhow::bail!("git pull failed in {}", local_path.display());
        }
    } else {
        eprintln!("Cloning {} into {} ...", url, local_path.display());
        let status = Command::new("git")
            .args(["clone", "--depth=1", url, local_path.to_str().unwrap()])
            .status()
            .with_context(|| format!("Failed to clone {}", url))?;
        if !status.success() {
            anyhow::bail!("git clone failed for {}", url);
        }
    }
    Ok(())
}

fn collect_csswg(csswg_dir: &Path) -> Vec<SpecEntry> {
    let mut entries = Vec::new();
    let skip = ["bin", "css-module"];
    let read_dir = match std::fs::read_dir(csswg_dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("warning: cannot read {}: {}", csswg_dir.display(), e);
            return entries;
        }
    };
    let mut dirs: Vec<_> = read_dir.flatten().filter(|e| e.path().is_dir()).collect();
    dirs.sort_by_key(|e| e.file_name());

    for entry in dirs {
        let dir_name = entry.file_name();
        let dir_name = dir_name.to_string_lossy();
        if dir_name.starts_with('.') || skip.contains(&dir_name.as_ref()) {
            continue;
        }
        let has_bs = std::fs::read_dir(entry.path())
            .ok()
            .map(|rd| {
                rd.flatten()
                    .any(|f| f.file_name().to_string_lossy().ends_with(".bs"))
            })
            .unwrap_or(false);
        if !has_bs {
            continue;
        }
        entries.push(SpecEntry {
            name: dir_name.to_uppercase(),
            base_url: format!("https://drafts.csswg.org/{}", dir_name),
            provider: "w3c".to_string(),
            github_repo: "w3c/csswg-drafts".to_string(),
        });
    }
    entries
}

fn collect_standalone(groups_dir: &Path) -> Result<Vec<SpecEntry>> {
    let repos_path = groups_dir.join("repositories.json");
    let data = std::fs::read_to_string(&repos_path)
        .with_context(|| format!("Failed to read {}", repos_path.display()))?;
    let repos: Vec<serde_json::Value> =
        serde_json::from_str(&data).context("Failed to parse repositories.json")?;

    let mut entries = Vec::new();
    for r in &repos {
        if r.get("isArchived")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        if r.get("isPrivate")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        let types: Vec<&str> = r
            .get("w3cjson")
            .and_then(|v| v.get("repo-type"))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if !types.contains(&"rec-track") && !types.contains(&"cg-report") {
            continue;
        }
        let owner = r
            .get("owner")
            .and_then(|v| v.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let repo_name = r.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if owner.is_empty() || repo_name.is_empty() {
            continue;
        }
        if owner == "w3c" && repo_name == "csswg-drafts" {
            continue;
        }

        let hp_raw = r
            .get("homepageUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim_end_matches('/')
            .replace("http://", "https://");
        let hp = if !hp_raw.is_empty() && !hp_raw.starts_with("https://") {
            format!("https://{}", hp_raw)
        } else {
            hp_raw
        };

        let base_url = if hp.contains(".github.io") && !hp.ends_with(".github.io") {
            hp
        } else if owner == "w3c" && (hp.starts_with("https://www.w3.org/TR/") || hp.is_empty()) {
            format!("https://w3c.github.io/{}", repo_name)
        } else {
            continue;
        };

        entries.push(SpecEntry {
            name: repo_name.to_uppercase(),
            base_url,
            provider: "w3c".to_string(),
            github_repo: format!("{}/{}", owner, repo_name),
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_repo(
        owner: &str,
        name: &str,
        homepage: &str,
        repo_types: &[&str],
    ) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "owner": {"login": owner},
            "homepageUrl": homepage,
            "isArchived": false,
            "isPrivate": false,
            "w3cjson": {
                "repo-type": repo_types
            }
        })
    }

    #[test]
    fn test_collect_standalone_github_io_url() {
        let repos = serde_json::json!([make_repo(
            "w3c",
            "webcodecs",
            "https://w3c.github.io/webcodecs/",
            &["rec-track"]
        )]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repositories.json");
        std::fs::write(&path, repos.to_string()).unwrap();
        let entries = collect_standalone(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "WEBCODECS");
        assert_eq!(entries[0].base_url, "https://w3c.github.io/webcodecs");
        assert_eq!(entries[0].github_repo, "w3c/webcodecs");
    }

    #[test]
    fn test_collect_standalone_tr_url_becomes_github_io() {
        let repos = serde_json::json!([make_repo(
            "w3c",
            "permissions",
            "https://www.w3.org/TR/permissions/",
            &["rec-track"]
        )]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repositories.json");
        std::fs::write(&path, repos.to_string()).unwrap();
        let entries = collect_standalone(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_url, "https://w3c.github.io/permissions");
    }

    #[test]
    fn test_collect_standalone_bare_hostname_gets_https() {
        let repos = serde_json::json!([make_repo(
            "w3c",
            "rdf-tests",
            "w3c.github.io/rdf-tests",
            &["rec-track"]
        )]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repositories.json");
        std::fs::write(&path, repos.to_string()).unwrap();
        let entries = collect_standalone(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_url, "https://w3c.github.io/rdf-tests");
    }

    #[test]
    fn test_collect_standalone_skips_archived() {
        let mut r = make_repo(
            "w3c",
            "old-spec",
            "https://w3c.github.io/old-spec/",
            &["rec-track"],
        );
        r["isArchived"] = serde_json::json!(true);
        let repos = serde_json::json!([r]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("repositories.json"), repos.to_string()).unwrap();
        let entries = collect_standalone(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_collect_standalone_skips_non_spec_types() {
        let repos = serde_json::json!([
            make_repo("w3c", "tests", "https://w3c.github.io/tests/", &["tests"]),
            make_repo("w3c", "tool", "https://w3c.github.io/tool/", &["tool"]),
        ]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("repositories.json"), repos.to_string()).unwrap();
        let entries = collect_standalone(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_collect_standalone_includes_cg_report() {
        let repos = serde_json::json!([make_repo(
            "WICG",
            "keyboard-lock",
            "https://wicg.github.io/keyboard-lock/",
            &["cg-report"]
        )]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("repositories.json"), repos.to_string()).unwrap();
        let entries = collect_standalone(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "KEYBOARD-LOCK");
    }

    #[test]
    fn test_collect_standalone_skips_csswg_monorepo() {
        let repos = serde_json::json!([make_repo(
            "w3c",
            "csswg-drafts",
            "https://drafts.csswg.org/index.html",
            &["rec-track"]
        )]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("repositories.json"), repos.to_string()).unwrap();
        let entries = collect_standalone(dir.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_resolve_collisions_disambiguates_by_org() {
        let mut entries = vec![
            SpecEntry {
                name: "SPEC".into(),
                base_url: "https://foo.github.io/spec".into(),
                provider: "w3c".into(),
                github_repo: "foo/spec".into(),
            },
            SpecEntry {
                name: "SPEC".into(),
                base_url: "https://bar.github.io/spec".into(),
                provider: "w3c".into(),
                github_repo: "bar/spec".into(),
            },
        ];
        resolve_collisions(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"SPEC-FOO"));
        assert!(names.contains(&"SPEC-BAR"));
    }

    #[test]
    fn test_no_duplicate_names_or_urls_in_generated_list() {
        let data = std::fs::read_to_string("data/w3c_specs.json");
        if data.is_err() {
            return; // Skip if not generated yet
        }
        let specs: Vec<SpecEntry> = serde_json::from_str(&data.unwrap()).unwrap();
        let mut names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(
            names.len(),
            before,
            "Duplicate names in data/w3c_specs.json"
        );

        let mut urls: Vec<&str> = specs.iter().map(|s| s.base_url.as_str()).collect();
        urls.sort();
        let before = urls.len();
        urls.dedup();
        assert_eq!(
            urls.len(),
            before,
            "Duplicate base_urls in data/w3c_specs.json"
        );
    }

    #[test]
    fn test_generated_list_all_https() {
        let data = std::fs::read_to_string("data/w3c_specs.json");
        if data.is_err() {
            return;
        }
        let specs: Vec<SpecEntry> = serde_json::from_str(&data.unwrap()).unwrap();
        for s in &specs {
            assert!(
                s.base_url.starts_with("https://"),
                "Non-https URL in data/w3c_specs.json: {} -> {}",
                s.name,
                s.base_url
            );
        }
    }
}

fn resolve_collisions(entries: &mut [SpecEntry]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for e in entries.iter() {
        *counts.entry(e.name.clone()).or_insert(0) += 1;
    }
    for e in entries.iter_mut() {
        if counts[&e.name] > 1 {
            let org = e.github_repo.split('/').next().unwrap_or("").to_uppercase();
            e.name = format!("{}-{}", e.name, org);
        }
    }
    // Second pass for remaining collisions
    let mut counts: HashMap<String, usize> = HashMap::new();
    for e in entries.iter() {
        *counts.entry(e.name.clone()).or_insert(0) += 1;
    }
    for e in entries.iter_mut() {
        if counts[&e.name] > 1 {
            e.name = e.github_repo.replace('/', "-").to_uppercase();
        }
    }
}
