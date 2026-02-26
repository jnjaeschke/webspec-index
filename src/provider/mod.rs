pub mod tc39;
pub mod w3c;
pub mod whatwg;

use crate::model::SpecInfo;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Trait for spec providers (WHATWG, W3C, TC39, etc.)
#[async_trait]
pub trait SpecProvider {
    /// Short name for this provider: "whatwg", "w3c", "tc39"
    fn provider_name(&self) -> &str;

    /// List all specs this provider knows about
    fn known_specs(&self) -> &[SpecInfo];

    /// Fetch the rendered HTML for a spec at a given version
    async fn fetch_html(&self, spec: &SpecInfo, sha: &str) -> Result<String>;

    /// Fetch the latest version identifier (SHA) and its commit date
    async fn fetch_latest_version(&self, spec: &SpecInfo) -> Result<(String, DateTime<Utc>)>;

    /// Map a URL found in an <a href> to (spec_name, anchor), if recognized
    fn resolve_url(&self, url: &str) -> Option<(String, String)>;
}
