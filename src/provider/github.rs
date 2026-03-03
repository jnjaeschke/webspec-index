use super::SpecAccess;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

const USER_AGENT: &str = concat!("webspec-index/", env!("CARGO_PKG_VERSION"));

pub struct GithubSpecInfo {
    pub name: String,
    pub url: String,
    pub provider: String,
    pub github_repo: String,
    pub html_url_template: String,
    pub commit_history_url: String,
}

#[async_trait]
impl SpecAccess for GithubSpecInfo {
    fn name(&self) -> &str {
        &self.name
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn provider(&self) -> &str {
        &self.provider
    }

    fn version_cache_key(&self) -> &str {
        &self.github_repo
    }

    async fn fetch_html(&self, sha: &str) -> Result<String> {
        let url = self.html_url_template.replace("{sha}", sha);
        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch {}: HTTP {}", url, response.status());
        }

        Ok(response.text().await?)
    }

    async fn fetch_latest_version(&self) -> Result<(String, DateTime<Utc>)> {
        let client = reqwest::Client::new();
        let response = client
            .get(&self.commit_history_url)
            .header("User-Agent", USER_AGENT)
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

    async fn fetch_version_date(&self, sha: &str) -> Result<DateTime<Utc>> {
        let url = format!(
            "https://api.github.com/repos/{}/commits/{}",
            self.github_repo, sha
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch commit {}: HTTP {}", sha, response.status());
        }

        let commit: serde_json::Value = response.json().await?;
        let date_str = commit["commit"]["committer"]["date"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing date in commit"))?;

        let date = DateTime::parse_from_rfc3339(date_str)?.with_timezone(&Utc);

        Ok(date)
    }
}
