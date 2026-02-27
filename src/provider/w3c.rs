use super::SpecProvider;
use crate::model::SpecInfo;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub struct W3cProvider;

// Registry of known W3C specs.
// Two flavors:
//   - CSSWG specs hosted at drafts.csswg.org (monorepo: w3c/csswg-drafts)
//   - Standalone specs hosted at w3c.github.io (individual repos)
pub const W3C_SPECS: &[SpecInfo] = &[
    // --- CSSWG specs (monorepo: w3c/csswg-drafts) ---
    SpecInfo {
        name: "CSS-ALIGN",
        base_url: "https://drafts.csswg.org/css-align-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-ANCHOR-POSITION",
        base_url: "https://drafts.csswg.org/css-anchor-position-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-ANIMATIONS",
        base_url: "https://drafts.csswg.org/css-animations-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-BACKGROUNDS",
        base_url: "https://drafts.csswg.org/css-backgrounds-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-BOX",
        base_url: "https://drafts.csswg.org/css-box-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-BREAK",
        base_url: "https://drafts.csswg.org/css-break-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-CASCADE",
        base_url: "https://drafts.csswg.org/css-cascade-6",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-COLOR",
        base_url: "https://drafts.csswg.org/css-color-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-COLOR-ADJUST",
        base_url: "https://drafts.csswg.org/css-color-adjust-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-COMPOSITING",
        base_url: "https://drafts.csswg.org/compositing-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-CONDITIONAL",
        base_url: "https://drafts.csswg.org/css-conditional-5",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-CONTAIN",
        base_url: "https://drafts.csswg.org/css-contain-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-COUNTER-STYLES",
        base_url: "https://drafts.csswg.org/css-counter-styles-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-DISPLAY",
        base_url: "https://drafts.csswg.org/css-display-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-EASING",
        base_url: "https://drafts.csswg.org/css-easing-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-FILTER-EFFECTS",
        base_url: "https://drafts.csswg.org/filter-effects-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-FLEXBOX",
        base_url: "https://drafts.csswg.org/css-flexbox-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-FONT-LOADING",
        base_url: "https://drafts.csswg.org/css-font-loading-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-FONTS",
        base_url: "https://drafts.csswg.org/css-fonts-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-GRID",
        base_url: "https://drafts.csswg.org/css-grid-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-HIGHLIGHT-API",
        base_url: "https://drafts.csswg.org/css-highlight-api-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-IMAGES",
        base_url: "https://drafts.csswg.org/css-images-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-INLINE",
        base_url: "https://drafts.csswg.org/css-inline-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-LISTS",
        base_url: "https://drafts.csswg.org/css-lists-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-LOGICAL",
        base_url: "https://drafts.csswg.org/css-logical-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-MASKING",
        base_url: "https://drafts.csswg.org/css-masking-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-MEDIAQUERIES",
        base_url: "https://drafts.csswg.org/mediaqueries-5",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-MOTION",
        base_url: "https://drafts.csswg.org/motion-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-MULTICOL",
        base_url: "https://drafts.csswg.org/css-multicol-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-NESTING",
        base_url: "https://drafts.csswg.org/css-nesting-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-OVERFLOW",
        base_url: "https://drafts.csswg.org/css-overflow-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-OVERSCROLL",
        base_url: "https://drafts.csswg.org/css-overscroll-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-PAGE",
        base_url: "https://drafts.csswg.org/css-page-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-POSITION",
        base_url: "https://drafts.csswg.org/css-position-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-PSEUDO",
        base_url: "https://drafts.csswg.org/css-pseudo-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-RUBY",
        base_url: "https://drafts.csswg.org/css-ruby-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SCROLL-ANCHORING",
        base_url: "https://drafts.csswg.org/css-scroll-anchoring-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SCROLL-SNAP",
        base_url: "https://drafts.csswg.org/css-scroll-snap-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SCROLLBARS",
        base_url: "https://drafts.csswg.org/css-scrollbars-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SELECTORS",
        base_url: "https://drafts.csswg.org/selectors-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SHADOW-PARTS",
        base_url: "https://drafts.csswg.org/css-shadow-parts-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SHAPES",
        base_url: "https://drafts.csswg.org/css-shapes-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SIZING",
        base_url: "https://drafts.csswg.org/css-sizing-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-SYNTAX",
        base_url: "https://drafts.csswg.org/css-syntax-3",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-TEXT",
        base_url: "https://drafts.csswg.org/css-text-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-TEXT-DECOR",
        base_url: "https://drafts.csswg.org/css-text-decor-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-TRANSFORMS",
        base_url: "https://drafts.csswg.org/css-transforms-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-TRANSITIONS",
        base_url: "https://drafts.csswg.org/css-transitions-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-UI",
        base_url: "https://drafts.csswg.org/css-ui-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-VALUES",
        base_url: "https://drafts.csswg.org/css-values-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-VARIABLES",
        base_url: "https://drafts.csswg.org/css-variables-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-VIEW-TRANSITIONS",
        base_url: "https://drafts.csswg.org/css-view-transitions-2",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-WILL-CHANGE",
        base_url: "https://drafts.csswg.org/css-will-change-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSS-WRITING-MODES",
        base_url: "https://drafts.csswg.org/css-writing-modes-4",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSSOM",
        base_url: "https://drafts.csswg.org/cssom-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "CSSOM-VIEW",
        base_url: "https://drafts.csswg.org/cssom-view-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "GEOMETRY",
        base_url: "https://drafts.csswg.org/geometry-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "RESIZE-OBSERVER",
        base_url: "https://drafts.csswg.org/resize-observer-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "SCROLL-ANIMATIONS",
        base_url: "https://drafts.csswg.org/scroll-animations-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    SpecInfo {
        name: "WEB-ANIMATIONS",
        base_url: "https://drafts.csswg.org/web-animations-1",
        provider: "w3c",
        github_repo: "w3c/csswg-drafts",
    },
    // --- Standalone W3C specs (individual repos) ---
    SpecInfo {
        name: "FILE-API",
        base_url: "https://w3c.github.io/FileAPI",
        provider: "w3c",
        github_repo: "w3c/FileAPI",
    },
    SpecInfo {
        name: "PERMISSIONS",
        base_url: "https://w3c.github.io/permissions",
        provider: "w3c",
        github_repo: "w3c/permissions",
    },
    SpecInfo {
        name: "POINTER-EVENTS",
        base_url: "https://w3c.github.io/pointerevents",
        provider: "w3c",
        github_repo: "w3c/pointerevents",
    },
    SpecInfo {
        name: "SERVICE-WORKERS",
        base_url: "https://w3c.github.io/ServiceWorker",
        provider: "w3c",
        github_repo: "w3c/ServiceWorker",
    },
    SpecInfo {
        name: "WEBCODECS",
        base_url: "https://w3c.github.io/webcodecs",
        provider: "w3c",
        github_repo: "w3c/webcodecs",
    },
];

/// Extract the CSSWG spec directory name from a base URL.
/// Returns None for non-CSSWG (standalone) specs.
fn csswg_spec_dir(spec: &SpecInfo) -> Option<&str> {
    spec.base_url.strip_prefix("https://drafts.csswg.org/")
}

#[async_trait]
impl SpecProvider for W3cProvider {
    fn provider_name(&self) -> &str {
        "w3c"
    }

    fn known_specs(&self) -> &[SpecInfo] {
        W3C_SPECS
    }

    /// Fetch the rendered HTML for a W3C spec.
    /// Always fetches the current editor's draft (SHA parameter is ignored since
    /// W3C specs don't have commit-snapshot URLs like WHATWG).
    async fn fetch_html(&self, spec: &SpecInfo, _sha: &str) -> Result<String> {
        let url = format!("{}/", spec.base_url.trim_end_matches('/'));

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "webspec-index/0.3.0")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch {}: HTTP {}", url, response.status());
        }

        Ok(response.text().await?)
    }

    /// Fetch the latest commit SHA for the spec's GitHub repo.
    /// For CSSWG monorepo specs, returns the monorepo HEAD (no path filter),
    /// so all CSSWG specs share one API call via the repo-level cache.
    async fn fetch_latest_version(&self, spec: &SpecInfo) -> Result<(String, DateTime<Utc>)> {
        let url = format!(
            "https://api.github.com/repos/{}/commits?per_page=1",
            spec.github_repo
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "webspec-index/0.3.0")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch latest commit: HTTP {}", response.status());
        }

        let commits: serde_json::Value = response.json().await?;
        let commit = commits
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow::anyhow!("No commits found for {}", spec.name))?;

        let sha = commit["sha"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing SHA in commit"))?
            .to_string();

        let date_str = commit["commit"]["committer"]["date"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing date in commit"))?;

        let date = DateTime::parse_from_rfc3339(date_str)?.with_timezone(&Utc);

        Ok((sha, date))
    }

    fn resolve_url(&self, url: &str) -> Option<(String, String)> {
        let parsed = url::Url::parse(url).ok()?;
        let anchor = parsed.fragment()?.to_string();
        let host = parsed.host_str()?;

        match host {
            "drafts.csswg.org" => {
                let path = parsed.path().trim_matches('/');
                for spec in W3C_SPECS {
                    if let Some(dir) = csswg_spec_dir(spec) {
                        if dir == path {
                            return Some((spec.name.to_string(), anchor));
                        }
                    }
                }
                None
            }
            "w3c.github.io" => {
                // Path might be /ServiceWorker/ or /ServiceWorker/v1/ — match on first segment
                let repo_part = parsed.path().trim_matches('/').split('/').next()?;
                for spec in W3C_SPECS {
                    if spec.base_url == format!("https://w3c.github.io/{}", repo_part) {
                        return Some((spec.name.to_string(), anchor));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- csswg_spec_dir helper --

    #[test]
    fn test_csswg_spec_dir_extraction() {
        let spec = &W3C_SPECS
            .iter()
            .find(|s| s.name == "CSS-SELECTORS")
            .unwrap();
        assert_eq!(csswg_spec_dir(spec), Some("selectors-4"));
    }

    #[test]
    fn test_csswg_spec_dir_standalone_returns_none() {
        let spec = &W3C_SPECS
            .iter()
            .find(|s| s.name == "SERVICE-WORKERS")
            .unwrap();
        assert_eq!(csswg_spec_dir(spec), None);
    }

    // -- resolve_url --

    #[test]
    fn test_resolve_csswg_url() {
        let provider = W3cProvider;
        let result = provider.resolve_url("https://drafts.csswg.org/selectors-4/#specificity");
        assert_eq!(
            result,
            Some(("CSS-SELECTORS".to_string(), "specificity".to_string()))
        );
    }

    #[test]
    fn test_resolve_csswg_url_css_display() {
        let provider = W3cProvider;
        let result =
            provider.resolve_url("https://drafts.csswg.org/css-display-4/#propdef-display");
        assert_eq!(
            result,
            Some(("CSS-DISPLAY".to_string(), "propdef-display".to_string()))
        );
    }

    #[test]
    fn test_resolve_csswg_url_with_trailing_slash() {
        let provider = W3cProvider;
        // URLs in specs sometimes have trailing slash before fragment
        let result = provider.resolve_url("https://drafts.csswg.org/css-values-4/#lengths");
        assert_eq!(
            result,
            Some(("CSS-VALUES".to_string(), "lengths".to_string()))
        );
    }

    #[test]
    fn test_resolve_standalone_url() {
        let provider = W3cProvider;
        let result =
            provider.resolve_url("https://w3c.github.io/ServiceWorker/#service-worker-concept");
        assert_eq!(
            result,
            Some((
                "SERVICE-WORKERS".to_string(),
                "service-worker-concept".to_string()
            ))
        );
    }

    #[test]
    fn test_resolve_standalone_url_permissions() {
        let provider = W3cProvider;
        let result = provider.resolve_url("https://w3c.github.io/permissions/#dfn-permission");
        assert_eq!(
            result,
            Some(("PERMISSIONS".to_string(), "dfn-permission".to_string()))
        );
    }

    #[test]
    fn test_resolve_unknown_csswg_url() {
        let provider = W3cProvider;
        let result = provider.resolve_url("https://drafts.csswg.org/not-indexed-spec/#foo");
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_unknown_standalone_url() {
        let provider = W3cProvider;
        let result = provider.resolve_url("https://w3c.github.io/not-indexed/#foo");
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_url_no_fragment() {
        let provider = W3cProvider;
        assert_eq!(
            provider.resolve_url("https://drafts.csswg.org/selectors-4/"),
            None
        );
        assert_eq!(
            provider.resolve_url("https://w3c.github.io/ServiceWorker/"),
            None
        );
    }

    #[test]
    fn test_resolve_external_url() {
        let provider = W3cProvider;
        assert_eq!(provider.resolve_url("https://example.com/#foo"), None);
        assert_eq!(
            provider.resolve_url("https://html.spec.whatwg.org/#navigate"),
            None
        );
    }

    // -- Spec registry invariants --

    #[test]
    fn test_no_duplicate_spec_names() {
        let mut names: Vec<&str> = W3C_SPECS.iter().map(|s| s.name).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "Duplicate spec names found");
    }

    #[test]
    fn test_no_duplicate_base_urls() {
        let mut urls: Vec<&str> = W3C_SPECS.iter().map(|s| s.base_url).collect();
        urls.sort();
        let before = urls.len();
        urls.dedup();
        assert_eq!(urls.len(), before, "Duplicate base URLs found");
    }

    #[test]
    fn test_all_specs_have_w3c_provider() {
        for spec in W3C_SPECS {
            assert_eq!(
                spec.provider, "w3c",
                "Spec {} has wrong provider: {}",
                spec.name, spec.provider
            );
        }
    }

    #[test]
    fn test_csswg_specs_use_monorepo() {
        for spec in W3C_SPECS {
            if spec.base_url.starts_with("https://drafts.csswg.org/") {
                assert_eq!(
                    spec.github_repo, "w3c/csswg-drafts",
                    "CSSWG spec {} should use monorepo",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn test_all_specs_have_valid_base_urls() {
        for spec in W3C_SPECS {
            assert!(
                spec.base_url.starts_with("https://drafts.csswg.org/")
                    || spec.base_url.starts_with("https://w3c.github.io/"),
                "Spec {} has unexpected base_url: {}",
                spec.name,
                spec.base_url
            );
            // No trailing slashes in base_url (consistency)
            assert!(
                !spec.base_url.ends_with('/'),
                "Spec {} base_url should not end with '/': {}",
                spec.name,
                spec.base_url
            );
        }
    }

    #[test]
    fn test_standalone_specs_have_matching_repo() {
        for spec in W3C_SPECS {
            if spec.base_url.starts_with("https://w3c.github.io/") {
                let repo_name = spec
                    .base_url
                    .strip_prefix("https://w3c.github.io/")
                    .unwrap();
                let expected_repo = format!("w3c/{}", repo_name);
                assert_eq!(
                    spec.github_repo, expected_repo,
                    "Standalone spec {} repo mismatch",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn test_no_name_clashes_with_whatwg() {
        use crate::provider::whatwg::WHATWG_SPECS;
        let whatwg_names: std::collections::HashSet<&str> =
            WHATWG_SPECS.iter().map(|s| s.name).collect();
        for spec in W3C_SPECS {
            assert!(
                !whatwg_names.contains(spec.name),
                "W3C spec name {} clashes with WHATWG",
                spec.name
            );
        }
    }
}
