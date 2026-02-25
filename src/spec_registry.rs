use crate::model::SpecInfo;
use crate::provider::{tc39::Tc39Provider, w3c::W3cProvider, whatwg::WhatwgProvider, SpecProvider};
use anyhow::Result;

/// Top-level registry that routes to appropriate providers
pub struct SpecRegistry {
    providers: Vec<Box<dyn SpecProvider + Send + Sync>>,
}

impl SpecRegistry {
    pub fn new() -> Self {
        Self {
            providers: vec![
                Box::new(WhatwgProvider),
                Box::new(W3cProvider),
                Box::new(Tc39Provider),
            ],
        }
    }

    /// Find a spec by name (case-insensitive)
    pub fn find_spec(&self, name: &str) -> Option<&SpecInfo> {
        let name_lower = name.to_lowercase();
        for provider in &self.providers {
            for spec in provider.known_specs() {
                if spec.name.to_lowercase() == name_lower {
                    return Some(spec);
                }
            }
        }
        None
    }

    /// Get the provider for a spec
    pub fn get_provider(&self, spec: &SpecInfo) -> Result<&(dyn SpecProvider + Send + Sync)> {
        for provider in &self.providers {
            if provider.provider_name() == spec.provider {
                return Ok(provider.as_ref());
            }
        }
        anyhow::bail!("No provider found for spec {}", spec.name)
    }

    /// List all known specs
    pub fn list_all_specs(&self) -> Vec<&SpecInfo> {
        let mut specs = Vec::new();
        for provider in &self.providers {
            specs.extend(provider.known_specs());
        }
        specs
    }

    /// Map a URL to (spec_name, anchor) if recognized
    pub fn resolve_url(&self, url: &str) -> Option<(String, String)> {
        for provider in &self.providers {
            if let Some(result) = provider.resolve_url(url) {
                return Some(result);
            }
        }
        None
    }
}

impl Default for SpecRegistry {
    fn default() -> Self {
        Self::new()
    }
}
