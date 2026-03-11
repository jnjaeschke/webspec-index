pub mod ietf;
pub mod tc39;
pub mod w3c;
pub mod whatwg;

use crate::model::SpecInfo;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Trait for spec providers (WHATWG, W3C, TC39, IETF, etc.)
#[async_trait]
pub trait SpecProvider {
    /// Short name for this provider: "whatwg", "w3c", "tc39", "ietf"
    fn provider_name(&self) -> &str;

    /// List all specs this provider knows about statically.
    /// Dynamic providers (e.g. IETF) return an empty slice.
    fn known_specs(&self) -> &[SpecInfo];

    /// Dynamically look up a spec by name (e.g. "RFC9110", "draft-touch-sne").
    /// Returns Ok(None) if the name is not handled by this provider.
    /// Default implementation returns Ok(None); override in dynamic providers.
    async fn find_dynamic_spec(&self, name: &str) -> Result<Option<SpecInfo>> {
        let _ = name;
        Ok(None)
    }

    /// Fetch the rendered HTML for a spec at a given version
    async fn fetch_html(&self, spec: &SpecInfo, sha: &str) -> Result<String>;

    /// Fetch the latest version identifier (SHA) and its commit date
    async fn fetch_latest_version(&self, spec: &SpecInfo) -> Result<(String, DateTime<Utc>)>;

    /// Map a URL found in an <a href> to (spec_name, anchor), if recognized
    fn resolve_url(&self, url: &str) -> Option<(String, String)>;
}
