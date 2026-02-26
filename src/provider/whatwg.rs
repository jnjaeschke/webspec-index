use super::SpecProvider;
use crate::model::SpecInfo;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub struct WhatwgProvider;

// Registry of known WHATWG living standards
// Full list: https://spec.whatwg.org/
pub const WHATWG_SPECS: &[SpecInfo] = &[
    SpecInfo {
        name: "COMPAT",
        base_url: "https://compat.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/compat",
    },
    SpecInfo {
        name: "COMPRESSION",
        base_url: "https://compression.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/compression",
    },
    SpecInfo {
        name: "CONSOLE",
        base_url: "https://console.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/console",
    },
    SpecInfo {
        name: "COOKIESTORE",
        base_url: "https://cookiestore.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/cookiestore",
    },
    SpecInfo {
        name: "DOM",
        base_url: "https://dom.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/dom",
    },
    SpecInfo {
        name: "ENCODING",
        base_url: "https://encoding.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/encoding",
    },
    SpecInfo {
        name: "FETCH",
        base_url: "https://fetch.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/fetch",
    },
    SpecInfo {
        name: "FS",
        base_url: "https://fs.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/fs",
    },
    SpecInfo {
        name: "FULLSCREEN",
        base_url: "https://fullscreen.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/fullscreen",
    },
    SpecInfo {
        name: "HTML",
        base_url: "https://html.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/html",
    },
    SpecInfo {
        name: "INFRA",
        base_url: "https://infra.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/infra",
    },
    SpecInfo {
        name: "MIMESNIFF",
        base_url: "https://mimesniff.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/mimesniff",
    },
    SpecInfo {
        name: "NOTIFICATIONS",
        base_url: "https://notifications.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/notifications",
    },
    SpecInfo {
        name: "QUIRKS",
        base_url: "https://quirks.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/quirks",
    },
    SpecInfo {
        name: "STORAGE",
        base_url: "https://storage.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/storage",
    },
    SpecInfo {
        name: "STREAMS",
        base_url: "https://streams.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/streams",
    },
    SpecInfo {
        name: "URL",
        base_url: "https://url.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/url",
    },
    SpecInfo {
        name: "URLPATTERN",
        base_url: "https://urlpattern.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/urlpattern",
    },
    SpecInfo {
        name: "WEBIDL",
        base_url: "https://webidl.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/webidl",
    },
    SpecInfo {
        name: "WEBSOCKETS",
        base_url: "https://websockets.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/websockets",
    },
    SpecInfo {
        name: "XHR",
        base_url: "https://xhr.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/xhr",
    },
];

#[async_trait]
impl SpecProvider for WhatwgProvider {
    fn provider_name(&self) -> &str {
        "whatwg"
    }

    fn known_specs(&self) -> &[SpecInfo] {
        WHATWG_SPECS
    }

    async fn fetch_html(&self, spec: &SpecInfo, sha: &str) -> Result<String> {
        let url = format!("{}/commit-snapshots/{}/", spec.base_url, sha);
        let response = reqwest::get(&url).await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch {}: HTTP {}", url, response.status());
        }

        Ok(response.text().await?)
    }

    async fn fetch_latest_version(&self, spec: &SpecInfo) -> Result<(String, DateTime<Utc>)> {
        let url = format!(
            "https://api.github.com/repos/{}/commits?per_page=1",
            spec.github_repo
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "webspec-index/0.1.0")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch latest commit: HTTP {}", response.status());
        }

        let commits: serde_json::Value = response.json().await?;
        let commit = commits
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| anyhow::anyhow!("No commits found"))?;

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
        // Parse URL and match against known specs
        let url = url::Url::parse(url).ok()?;
        let base = format!("{}://{}", url.scheme(), url.host_str()?);

        for spec in WHATWG_SPECS {
            if spec.base_url == base {
                let anchor = url.fragment()?.to_string();
                return Some((spec.name.to_string(), anchor));
            }
        }

        None
    }
}
