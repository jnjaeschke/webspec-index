// Specs published by the "GPU for the Web" (gpuweb) organization
//
// These are similar to what `w3c::github_io_spec` offers, but there are a few
// differences:
//
// 1. gpuweb fetches versioned spec HTML by subsituting a `{sha}` placeholder.
//    github_io_spec produces a template with no `{sha}`, so it always fetches
//    the living standard.
// 2. gpuweb's `commit_history_url` includes `?sha=gh-pages` to track the built
//    output branch. github_io_spec tracks the default branch.
// 3. WGSL lives at `gpuweb.github.io/gpuweb/wgsl/`, a subpath of the main
//    gpuweb/gpuweb repo. github_io_spec assumes one repo per spec.

use super::{github::GithubSpecInfo, SpecAccess};

const GH_PAGES_BRANCH: &str = "gh-pages";

fn make_spec(name: &str, path: Option<&str>) -> Box<dyn SpecAccess> {
    let path = path.unwrap_or("");
    Box::new(GithubSpecInfo {
        name: name.into(),
        url: format!("https://gpuweb.github.io/gpuweb/{path}"),
        provider: "gpuweb".into(),
        github_repo: "gpuweb/gpuweb".into(),
        html_url_template: format!("https://github.com/gpuweb/gpuweb/raw/{{sha}}/{path}index.html"),
        commit_history_url: format!(
            "https://api.github.com/repos/gpuweb/gpuweb/commits?sha={GH_PAGES_BRANCH}&per_page=1",
        ),
    })
}

pub fn specs() -> Vec<Box<dyn super::SpecAccess>> {
    [("WEBGPU", None), ("WGSL", Some("wgsl/"))]
        .iter()
        .copied()
        .map(|(name, path)| make_spec(name, path))
        .collect()
}
