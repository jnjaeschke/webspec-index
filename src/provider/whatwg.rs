use super::SpecProvider;
use crate::model::SpecInfo;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub struct WhatwgProvider;

// Registry of known WHATWG specs
pub const WHATWG_SPECS: &[SpecInfo] = &[
    SpecInfo {
        name: "HTML",
        base_url: "https://html.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/html",
    },
    SpecInfo {
        name: "DOM",
        base_url: "https://dom.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/dom",
    },
    SpecInfo {
        name: "URL",
        base_url: "https://url.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/url",
    },
    SpecInfo {
        name: "INFRA",
        base_url: "https://infra.spec.whatwg.org",
        provider: "whatwg",
        github_repo: "whatwg/infra",
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
        let url = format!("https://api.github.com/repos/{}/commits?per_page=1", spec.github_repo);

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

        let date = DateTime::parse_from_rfc3339(date_str)?
            .with_timezone(&Utc);

        Ok((sha, date))
    }

    async fn fetch_version_date(&self, spec: &SpecInfo, sha: &str) -> Result<DateTime<Utc>> {
        let url = format!("https://api.github.com/repos/{}/commits/{}", spec.github_repo, sha);

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "webspec-index/0.1.0")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch commit {}: HTTP {}", sha, response.status());
        }

        let commit: serde_json::Value = response.json().await?;
        let date_str = commit["commit"]["committer"]["date"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing date in commit"))?;

        let date = DateTime::parse_from_rfc3339(date_str)?
            .with_timezone(&Utc);

        Ok(date)
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
