use super::{github::GithubSpecInfo, SpecAccess};

fn csswg_spec(name: &str, dir: &str) -> Box<dyn SpecAccess> {
    let base_url = format!("https://drafts.csswg.org/{dir}");
    Box::new(GithubSpecInfo {
        name: name.into(),
        url: base_url.clone(),
        provider: "w3c".into(),
        github_repo: "w3c/csswg-drafts".into(),
        html_url_template: format!("{base_url}/"),
        commit_history_url:
            "https://api.github.com/repos/w3c/csswg-drafts/commits?per_page=1".into(),
    })
}

fn github_io_spec(name: &str, org: &str, repo: &str) -> Box<dyn SpecAccess> {
    let base_url = format!("https://{org}.github.io/{repo}");
    Box::new(GithubSpecInfo {
        name: name.into(),
        url: base_url.clone(),
        provider: "w3c".into(),
        github_repo: format!("{org}/{repo}"),
        html_url_template: format!("{base_url}/"),
        commit_history_url: format!(
            "https://api.github.com/repos/{org}/{repo}/commits?per_page=1"
        ),
    })
}

// --- CSSWG specs (monorepo: w3c/csswg-drafts) ---
const CSSWG_SPECS: &[(&str, &str)] = &[
    ("CSS-ALIGN", "css-align-3"),
    ("CSS-ANCHOR-POSITION", "css-anchor-position-1"),
    ("CSS-ANIMATIONS", "css-animations-2"),
    ("CSS-BACKGROUNDS", "css-backgrounds-3"),
    ("CSS-BOX", "css-box-4"),
    ("CSS-BREAK", "css-break-4"),
    ("CSS-CASCADE", "css-cascade-6"),
    ("CSS-COLOR", "css-color-4"),
    ("CSS-COLOR-ADJUST", "css-color-adjust-1"),
    ("CSS-COMPOSITING", "compositing-1"),
    ("CSS-CONDITIONAL", "css-conditional-5"),
    ("CSS-CONTAIN", "css-contain-3"),
    ("CSS-COUNTER-STYLES", "css-counter-styles-3"),
    ("CSS-DISPLAY", "css-display-4"),
    ("CSS-EASING", "css-easing-2"),
    ("CSS-FILTER-EFFECTS", "filter-effects-2"),
    ("CSS-FLEXBOX", "css-flexbox-1"),
    ("CSS-FONT-LOADING", "css-font-loading-3"),
    ("CSS-FONTS", "css-fonts-4"),
    ("CSS-GRID", "css-grid-2"),
    ("CSS-HIGHLIGHT-API", "css-highlight-api-1"),
    ("CSS-IMAGES", "css-images-4"),
    ("CSS-INLINE", "css-inline-3"),
    ("CSS-LISTS", "css-lists-3"),
    ("CSS-LOGICAL", "css-logical-1"),
    ("CSS-MASKING", "css-masking-1"),
    ("CSS-MEDIAQUERIES", "mediaqueries-5"),
    ("CSS-MOTION", "motion-1"),
    ("CSS-MULTICOL", "css-multicol-1"),
    ("CSS-NESTING", "css-nesting-1"),
    ("CSS-OVERFLOW", "css-overflow-4"),
    ("CSS-OVERSCROLL", "css-overscroll-1"),
    ("CSS-PAGE", "css-page-3"),
    ("CSS-POSITION", "css-position-4"),
    ("CSS-PSEUDO", "css-pseudo-4"),
    ("CSS-RUBY", "css-ruby-1"),
    ("CSS-SCROLL-ANCHORING", "css-scroll-anchoring-1"),
    ("CSS-SCROLL-SNAP", "css-scroll-snap-2"),
    ("CSS-SCROLLBARS", "css-scrollbars-1"),
    ("CSS-SELECTORS", "selectors-4"),
    ("CSS-SHADOW-PARTS", "css-shadow-parts-1"),
    ("CSS-SHAPES", "css-shapes-1"),
    ("CSS-SIZING", "css-sizing-4"),
    ("CSS-SYNTAX", "css-syntax-3"),
    ("CSS-TEXT", "css-text-4"),
    ("CSS-TEXT-DECOR", "css-text-decor-4"),
    ("CSS-TRANSFORMS", "css-transforms-2"),
    ("CSS-TRANSITIONS", "css-transitions-2"),
    ("CSS-UI", "css-ui-4"),
    ("CSS-VALUES", "css-values-4"),
    ("CSS-VARIABLES", "css-variables-2"),
    ("CSS-VIEW-TRANSITIONS", "css-view-transitions-2"),
    ("CSS-WILL-CHANGE", "css-will-change-1"),
    ("CSS-WRITING-MODES", "css-writing-modes-4"),
    ("CSSOM", "cssom-1"),
    ("CSSOM-VIEW", "cssom-view-1"),
    ("GEOMETRY", "geometry-1"),
    ("RESIZE-OBSERVER", "resize-observer-1"),
    ("SCROLL-ANIMATIONS", "scroll-animations-1"),
    ("WEB-ANIMATIONS", "web-animations-1"),
];

// --- Standalone W3C specs (individual repos) ---
const W3C_STANDALONE: &[(&str, &str)] = &[
    ("FILE-API", "FileAPI"),
    ("PERMISSIONS", "permissions"),
    ("POINTER-EVENTS", "pointerevents"),
    ("SERVICE-WORKERS", "ServiceWorker"),
    ("WEBCODECS", "webcodecs"),
];

// --- webaudio GitHub org specs ---
const WEBAUDIO_SPECS: &[(&str, &str)] = &[
    ("WEB-AUDIO", "web-audio-api"),
    ("WEB-MIDI", "web-midi-api"),
    ("WEB-SPEECH", "web-speech-api"),
];

pub fn specs() -> Vec<Box<dyn SpecAccess>> {
    let mut out: Vec<Box<dyn SpecAccess>> = Vec::new();
    for &(name, dir) in CSSWG_SPECS {
        out.push(csswg_spec(name, dir));
    }
    for &(name, repo) in W3C_STANDALONE {
        out.push(github_io_spec(name, "w3c", repo));
    }
    for &(name, repo) in WEBAUDIO_SPECS {
        out.push(github_io_spec(name, "webaudio", repo));
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::spec_registry::SpecRegistry;

    // -- resolve_url --

    #[test]
    fn test_resolve_csswg_url() {
        let registry = SpecRegistry::new();
        let result = registry.resolve_url("https://drafts.csswg.org/selectors-4/#specificity");
        assert_eq!(
            result,
            Some(("CSS-SELECTORS".to_string(), "specificity".to_string()))
        );
    }

    #[test]
    fn test_resolve_csswg_url_css_display() {
        let registry = SpecRegistry::new();
        let result =
            registry.resolve_url("https://drafts.csswg.org/css-display-4/#propdef-display");
        assert_eq!(
            result,
            Some(("CSS-DISPLAY".to_string(), "propdef-display".to_string()))
        );
    }

    #[test]
    fn test_resolve_csswg_url_with_trailing_slash() {
        let registry = SpecRegistry::new();
        let result = registry.resolve_url("https://drafts.csswg.org/css-values-4/#lengths");
        assert_eq!(
            result,
            Some(("CSS-VALUES".to_string(), "lengths".to_string()))
        );
    }

    #[test]
    fn test_resolve_standalone_url() {
        let registry = SpecRegistry::new();
        let result =
            registry.resolve_url("https://w3c.github.io/ServiceWorker/#service-worker-concept");
        assert_eq!(
            result,
            Some((
                "SERVICE-WORKERS".to_string(),
                "service-worker-concept".to_string()
            ))
        );
    }

    #[test]
    fn test_resolve_standalone_url_permissions() {
        let registry = SpecRegistry::new();
        let result = registry.resolve_url("https://w3c.github.io/permissions/#dfn-permission");
        assert_eq!(
            result,
            Some(("PERMISSIONS".to_string(), "dfn-permission".to_string()))
        );
    }

    #[test]
    fn test_resolve_unknown_csswg_url() {
        let registry = SpecRegistry::new();
        let result = registry.resolve_url("https://drafts.csswg.org/not-indexed-spec/#foo");
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_unknown_standalone_url() {
        let registry = SpecRegistry::new();
        let result = registry.resolve_url("https://w3c.github.io/not-indexed/#foo");
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_webaudio_url() {
        let registry = SpecRegistry::new();
        let result =
            registry.resolve_url("https://webaudio.github.io/web-audio-api/#AudioContext");
        assert_eq!(
            result,
            Some(("WEB-AUDIO".to_string(), "AudioContext".to_string()))
        );
    }

    #[test]
    fn test_resolve_web_midi_url() {
        let registry = SpecRegistry::new();
        let result =
            registry.resolve_url("https://webaudio.github.io/web-midi-api/#MIDIAccess");
        assert_eq!(
            result,
            Some(("WEB-MIDI".to_string(), "MIDIAccess".to_string()))
        );
    }

    #[test]
    fn test_resolve_url_no_fragment() {
        let registry = SpecRegistry::new();
        assert_eq!(
            registry.resolve_url("https://drafts.csswg.org/selectors-4/"),
            None
        );
        assert_eq!(
            registry.resolve_url("https://w3c.github.io/ServiceWorker/"),
            None
        );
    }

    // -- Spec registry invariants --

    #[test]
    fn test_no_duplicate_spec_names() {
        let specs = super::specs();
        let mut names: Vec<&str> = specs.iter().map(|s| s.name()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "Duplicate spec names found");
    }

    #[test]
    fn test_no_duplicate_base_urls() {
        let specs = super::specs();
        let mut urls: Vec<&str> = specs.iter().map(|s| s.url()).collect();
        urls.sort();
        let before = urls.len();
        urls.dedup();
        assert_eq!(urls.len(), before, "Duplicate base URLs found");
    }

    #[test]
    fn test_all_specs_have_w3c_provider() {
        let specs = super::specs();
        for spec in &specs {
            assert_eq!(
                spec.provider(),
                "w3c",
                "Spec {} has wrong provider: {}",
                spec.name(),
                spec.provider()
            );
        }
    }

    #[test]
    fn test_csswg_specs_use_monorepo() {
        let specs = super::specs();
        for spec in &specs {
            if spec.url().starts_with("https://drafts.csswg.org/") {
                assert_eq!(
                    spec.version_cache_key(),
                    "w3c/csswg-drafts",
                    "CSSWG spec {} should use monorepo",
                    spec.name()
                );
            }
        }
    }

    #[test]
    fn test_all_specs_have_valid_base_urls() {
        let specs = super::specs();
        for spec in &specs {
            assert!(
                spec.url().starts_with("https://drafts.csswg.org/")
                    || spec.url().starts_with("https://w3c.github.io/")
                    || spec.url().starts_with("https://webaudio.github.io/"),
                "Spec {} has unexpected url: {}",
                spec.name(),
                spec.url()
            );
            assert!(
                !spec.url().ends_with('/'),
                "Spec {} url should not end with '/': {}",
                spec.name(),
                spec.url()
            );
        }
    }

    #[test]
    fn test_standalone_specs_have_matching_repo() {
        let specs = super::specs();
        for spec in &specs {
            if spec.url().starts_with("https://w3c.github.io/") {
                let repo_name = spec
                    .url()
                    .strip_prefix("https://w3c.github.io/")
                    .unwrap();
                let expected_repo = format!("w3c/{repo_name}");
                assert_eq!(
                    spec.version_cache_key(),
                    expected_repo,
                    "Standalone spec {} repo mismatch",
                    spec.name()
                );
            }
        }
    }

    #[test]
    fn test_no_name_clashes_with_whatwg() {
        let registry = SpecRegistry::new();
        let all_specs = registry.list_all_specs();
        let whatwg_names: std::collections::HashSet<&str> = all_specs
            .iter()
            .filter(|s| s.provider() == "whatwg")
            .map(|s| s.name())
            .collect();
        let w3c_specs = super::specs();
        for spec in &w3c_specs {
            assert!(
                !whatwg_names.contains(spec.name()),
                "W3C spec name {} clashes with WHATWG",
                spec.name()
            );
        }
    }
}
