use super::{github::GithubSpecInfo, SpecAccess};

pub fn specs() -> Vec<Box<dyn SpecAccess>> {
    vec![Box::new(GithubSpecInfo {
        name: "ECMA-262".into(),
        url: "https://tc39.es/ecma262".into(),
        provider: "tc39".into(),
        github_repo: "tc39/ecma262".into(),
        html_url_template: "https://tc39.es/ecma262/".into(),
        commit_history_url: "https://api.github.com/repos/tc39/ecma262/commits?per_page=1".into(),
    })]
}

#[cfg(test)]
mod tests {
    use crate::spec_registry::SpecRegistry;

    #[test]
    fn test_resolve_tc39_url() {
        let registry = SpecRegistry::new();
        let result = registry.resolve_url("https://tc39.es/ecma262/#sec-tostring");
        assert_eq!(
            result,
            Some(("ECMA-262".to_string(), "sec-tostring".to_string()))
        );
    }

    #[test]
    fn test_resolve_tc39_url_with_trailing_slash() {
        let registry = SpecRegistry::new();
        let result = registry.resolve_url("https://tc39.es/ecma262/#sec-object-type");
        assert_eq!(
            result,
            Some(("ECMA-262".to_string(), "sec-object-type".to_string()))
        );
    }

    #[test]
    fn test_resolve_tc39_url_no_fragment() {
        let registry = SpecRegistry::new();
        assert_eq!(registry.resolve_url("https://tc39.es/ecma262/"), None);
    }

    #[test]
    fn test_resolve_unknown_tc39_url() {
        let registry = SpecRegistry::new();
        assert_eq!(
            registry.resolve_url("https://tc39.es/proposal-temporal/#sec-foo"),
            None
        );
    }

    #[test]
    fn test_tc39_specs_have_correct_provider() {
        let specs = super::specs();
        for spec in &specs {
            assert_eq!(spec.provider(), "tc39");
        }
    }

    #[test]
    fn test_no_name_clashes() {
        let registry = SpecRegistry::new();
        let all_specs = registry.list_all_specs();
        let tc39_names: Vec<&str> = all_specs
            .iter()
            .filter(|s| s.provider() == "tc39")
            .map(|s| s.name())
            .collect();
        let other_names: Vec<&str> = all_specs
            .iter()
            .filter(|s| s.provider() != "tc39")
            .map(|s| s.name())
            .collect();
        for name in &tc39_names {
            assert!(
                !other_names.contains(name),
                "TC39 spec name {name} clashes with another provider"
            );
        }
    }
}
