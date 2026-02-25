pub mod github;
pub mod gpuweb;
pub mod tc39;
pub mod w3c;
pub mod whatwg;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Spec metadata + fetch behavior
#[async_trait]
pub trait SpecAccess: Send + Sync {
    /// Short name for this spec (e.g. "HTML", "DOM")
    fn name(&self) -> &str;

    /// Base URL for this spec (e.g. "https://html.spec.whatwg.org")
    fn url(&self) -> &str;

    /// Provider name (e.g. "whatwg", "w3c")
    fn provider(&self) -> &str;

    /// Cache key for repo-level SHA caching (e.g. "whatwg/html", "w3c/csswg-drafts")
    fn version_cache_key(&self) -> &str;

    /// Fetch the rendered HTML for a spec at a given version
    async fn fetch_html(&self, sha: &str) -> Result<String>;

    /// Fetch the latest version identifier (SHA) and its date
    async fn fetch_latest_version(&self) -> Result<(String, DateTime<Utc>)>;

    /// Fetch the commit date for a given version
    async fn fetch_version_date(&self, sha: &str) -> Result<DateTime<Utc>>;
}
