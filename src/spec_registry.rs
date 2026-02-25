use crate::provider::{tc39, w3c, whatwg, SpecAccess};

/// Top-level registry that routes to appropriate providers
pub struct SpecRegistry {
    specs: Vec<Box<dyn SpecAccess>>,
}

impl SpecRegistry {
    pub fn new() -> Self {
        let mut specs = whatwg::specs();
        specs.extend(w3c::specs());
        specs.extend(tc39::specs());
        Self { specs }
    }

    /// Find a spec by name (case-insensitive)
    pub fn find_spec(&self, name: &str) -> Option<&dyn SpecAccess> {
        let name_lower = name.to_lowercase();
        self.specs
            .iter()
            .find(|s| s.name().to_lowercase() == name_lower)
            .map(|s| s.as_ref())
    }

    /// List all known specs
    pub fn list_all_specs(&self) -> &[Box<dyn SpecAccess>] {
        &self.specs
    }

    /// Map a URL to (spec_name, anchor) if recognized
    pub fn resolve_url(&self, url: &str) -> Option<(String, String)> {
        let parsed = url::Url::parse(url).ok()?;
        let anchor = parsed.fragment()?.to_string();
        let mut without_fragment = parsed.clone();
        without_fragment.set_fragment(None);
        let base = without_fragment.as_str().trim_end_matches('/');

        for spec in &self.specs {
            let spec_url = spec.url().trim_end_matches('/');
            if base == spec_url || base.starts_with(&format!("{spec_url}/")) {
                return Some((spec.name().to_string(), anchor));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_unknown_url() {
        let registry = SpecRegistry::new();
        assert_eq!(registry.resolve_url("https://example.com/#foo"), None);
    }
}

impl Default for SpecRegistry {
    fn default() -> Self {
        Self::new()
    }
}
