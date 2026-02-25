use super::SpecProvider;
use crate::model::SpecInfo;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub struct Tc39Provider;

pub const TC39_SPECS: &[SpecInfo] = &[SpecInfo {
    name: "ECMA-262",
    base_url: "https://tc39.es/ecma262",
    provider: "tc39",
    github_repo: "tc39/ecma262",
}];

#[async_trait]
impl SpecProvider for Tc39Provider {
    fn provider_name(&self) -> &str {
        "tc39"
    }

    fn known_specs(&self) -> &[SpecInfo] {
        TC39_SPECS
    }

    /// Fetch the rendered HTML for a TC39 spec.
    /// Always fetches the current living standard (SHA parameter is ignored).
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

    async fn fetch_version_date(&self, spec: &SpecInfo, sha: &str) -> Result<DateTime<Utc>> {
        let url = format!(
            "https://api.github.com/repos/{}/commits/{}",
            spec.github_repo, sha
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "webspec-index/0.3.0")
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

    fn resolve_url(&self, url: &str) -> Option<(String, String)> {
        let parsed = url::Url::parse(url).ok()?;
        let anchor = parsed.fragment()?.to_string();
        let host = parsed.host_str()?;

        if host != "tc39.es" {
            return None;
        }

        // Match path: /ecma262 or /ecma262/
        let path = parsed.path().trim_matches('/');
        for spec in TC39_SPECS {
            let spec_path = spec
                .base_url
                .strip_prefix("https://tc39.es/")?;
            if path == spec_path {
                return Some((spec.name.to_string(), anchor));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_tc39_url() {
        let provider = Tc39Provider;
        let result = provider.resolve_url("https://tc39.es/ecma262/#sec-tostring");
        assert_eq!(
            result,
            Some(("ECMA-262".to_string(), "sec-tostring".to_string()))
        );
    }

    #[test]
    fn test_resolve_tc39_url_with_trailing_slash() {
        let provider = Tc39Provider;
        let result = provider.resolve_url("https://tc39.es/ecma262/#sec-object-type");
        assert_eq!(
            result,
            Some(("ECMA-262".to_string(), "sec-object-type".to_string()))
        );
    }

    #[test]
    fn test_resolve_tc39_url_no_fragment() {
        let provider = Tc39Provider;
        assert_eq!(provider.resolve_url("https://tc39.es/ecma262/"), None);
    }

    #[test]
    fn test_resolve_unknown_tc39_url() {
        let provider = Tc39Provider;
        // A proposal spec we don't index
        assert_eq!(
            provider.resolve_url("https://tc39.es/proposal-temporal/#sec-foo"),
            None
        );
    }

    #[test]
    fn test_resolve_external_url() {
        let provider = Tc39Provider;
        assert_eq!(provider.resolve_url("https://example.com/#foo"), None);
        assert_eq!(
            provider.resolve_url("https://html.spec.whatwg.org/#navigate"),
            None
        );
    }

    #[test]
    fn test_all_specs_have_tc39_provider() {
        for spec in TC39_SPECS {
            assert_eq!(spec.provider, "tc39");
        }
    }

    #[test]
    fn test_no_name_clashes() {
        use crate::provider::w3c::W3C_SPECS;
        use crate::provider::whatwg::WHATWG_SPECS;
        let mut all_names: Vec<&str> = Vec::new();
        all_names.extend(WHATWG_SPECS.iter().map(|s| s.name));
        all_names.extend(W3C_SPECS.iter().map(|s| s.name));
        for spec in TC39_SPECS {
            assert!(
                !all_names.contains(&spec.name),
                "TC39 spec name {} clashes with existing spec",
                spec.name
            );
        }
    }
}
