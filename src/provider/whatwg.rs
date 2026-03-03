use super::{github::GithubSpecInfo, SpecAccess};

fn make_spec(name: &str) -> Box<dyn SpecAccess> {
    let subdomain = name.to_ascii_lowercase();
    Box::new(GithubSpecInfo {
        name: name.into(),
        url: format!("https://{subdomain}.spec.whatwg.org"),
        provider: "whatwg".into(),
        github_repo: format!("whatwg/{subdomain}"),
        html_url_template: format!("https://{subdomain}.spec.whatwg.org/commit-snapshots/{{sha}}/"),
        commit_history_url: format!(
            "https://api.github.com/repos/whatwg/{subdomain}/commits?per_page=1"
        ),
    })
}

// Registry of known WHATWG living standards
// Full list: https://spec.whatwg.org/
pub fn specs() -> Vec<Box<dyn super::SpecAccess>> {
    [
        "COMPAT",
        "COMPRESSION",
        "CONSOLE",
        "COOKIESTORE",
        "DOM",
        "ENCODING",
        "FETCH",
        "FS",
        "FULLSCREEN",
        "HTML",
        "INFRA",
        "MIMESNIFF",
        "NOTIFICATIONS",
        "QUIRKS",
        "STORAGE",
        "STREAMS",
        "URL",
        "URLPATTERN",
        "WEBIDL",
        "WEBSOCKETS",
        "XHR",
    ]
    .iter()
    .map(|&name| make_spec(name))
    .collect()
}
