use anyhow::{Context, Result};

/// Parsed PR preview metadata extracted from a GitHub PR body.
#[derive(Debug, Clone)]
pub struct PrPreview {
    pub pr_number: i64,
    pub head_sha: String,
    pub merge_base_sha: String,
    pub pages: Vec<PrPage>,
}

/// A single preview page from whatpr.org.
#[derive(Debug, Clone)]
pub struct PrPage {
    pub page_path: String,
    pub url: String,
    pub diff_url: Option<String>,
}

/// Parse the preview block from a WHATWG PR body.
///
/// Expects the structured block below the `---` separator, containing
/// `<a href="https://whatpr.org/...">` links with commit SHAs in title attrs.
pub fn parse_pr_body(pr_number: i64, body: &str) -> Result<PrPreview> {
    let preview_block = body
        .split("Don't remove this comment or modify anything below this line.")
        .nth(1)
        .context("PR body has no preview block")?;

    let mut pages = Vec::new();
    let mut head_sha = String::new();
    let mut merge_base_sha = String::new();

    for line in preview_block.lines() {
        if let Some(url) = extract_href(line) {
            if !url.contains("whatpr.org") {
                continue;
            }
            // Skip diff links (they contain "..." in the path)
            if url.contains("...") {
                continue;
            }
            let page_path = url.rsplit('/').next().unwrap_or("").to_string();

            if head_sha.is_empty() {
                if let Some(sha) = extract_sha_from_title(line) {
                    head_sha = sha;
                }
            }

            let diff_url = extract_diff_url(line);
            if merge_base_sha.is_empty() {
                if let Some(ref du) = diff_url {
                    if let Some(base) = extract_merge_base_from_diff_url(du) {
                        merge_base_sha = base;
                    }
                }
            }

            pages.push(PrPage { page_path, url, diff_url });
        }
    }

    if pages.is_empty() {
        anyhow::bail!("No preview pages found in PR body");
    }
    if head_sha.is_empty() {
        anyhow::bail!("Could not extract head SHA from PR body");
    }
    if merge_base_sha.is_empty() {
        anyhow::bail!("Could not extract merge base SHA from PR body");
    }

    Ok(PrPreview { pr_number, head_sha, merge_base_sha, pages })
}

fn extract_href(line: &str) -> Option<String> {
    let idx = line.find("href=\"")?;
    let start = idx + 6;
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_sha_from_title(line: &str) -> Option<String> {
    // title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)"
    let title_idx = line.find("title=\"")?;
    let after_title = &line[title_idx + 7..];
    let title_end = after_title.find('"')?;
    let title_value = &after_title[..title_end];
    let paren_start = title_value.rfind('(')?;
    let paren_end = title_value.rfind(')')?;
    if paren_start < paren_end {
        Some(title_value[paren_start + 1..paren_end].to_string())
    } else {
        None
    }
}

fn extract_diff_url(line: &str) -> Option<String> {
    let mut search_from = 0;
    loop {
        let rest = &line[search_from..];
        let idx = rest.find("href=\"")?;
        let abs_idx = search_from + idx;
        let start = abs_idx + 6;
        let url_rest = &line[start..];
        let end = url_rest.find('"')?;
        let url = &url_rest[..end];
        if url.contains("...") && url.contains("whatpr.org") {
            return Some(url.to_string());
        }
        search_from = start + end;
        if search_from >= line.len() {
            return None;
        }
    }
}

/// Extract merge base SHA from diff URL.
/// URL format: https://whatpr.org/html/11741/74cbe0a...7ceff82/page.html
fn extract_merge_base_from_diff_url(url: &str) -> Option<String> {
    let parts: Vec<&str> = url.split('/').collect();
    for part in &parts {
        if part.contains("...") {
            return part.split("...").next().map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PR_BODY: &str = r#"Some PR description text here.

<!--
    This comment and the below content is programmatically generated.
    You may add a comma-separated list of anchors you'd like a
    direct link to below (e.g. #idl-serializers, #idl-sequence):

    Don't remove this comment or modify anything below this line.
    If you don't want a preview generated for this pull request,
    just replace the whole of this comment's content by "no preview"
    and remove what's below.
-->
***
<a href="https://whatpr.org/html/11741/form-control-infrastructure.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">/form-control-infrastructure.html</a>  ( <a href="https://whatpr.org/html/11741/74cbe0a...7ceff82/form-control-infrastructure.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">diff</a> )
<a href="https://whatpr.org/html/11741/form-elements.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">/form-elements.html</a>  ( <a href="https://whatpr.org/html/11741/74cbe0a...7ceff82/form-elements.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">diff</a> )
<a href="https://whatpr.org/html/11741/infrastructure.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">/infrastructure.html</a>  ( <a href="https://whatpr.org/html/11741/74cbe0a...7ceff82/infrastructure.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">diff</a> )
<a href="https://whatpr.org/html/11741/input.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">/input.html</a>  ( <a href="https://whatpr.org/html/11741/74cbe0a...7ceff82/input.html" title="Last updated on Apr 28, 2026, 4:05 PM UTC (7ceff82)">diff</a> )"#;

    #[test]
    fn test_parse_pr_body_extracts_pages() {
        let preview = parse_pr_body(11741, SAMPLE_PR_BODY).unwrap();
        assert_eq!(preview.pr_number, 11741);
        assert_eq!(preview.head_sha, "7ceff82");
        assert_eq!(preview.merge_base_sha, "74cbe0a");
        assert_eq!(preview.pages.len(), 4);
        assert_eq!(preview.pages[0].page_path, "form-control-infrastructure.html");
        assert_eq!(
            preview.pages[0].url,
            "https://whatpr.org/html/11741/form-control-infrastructure.html"
        );
        assert!(preview.pages[0].diff_url.is_some());
    }

    #[test]
    fn test_parse_pr_body_no_preview_block() {
        let result = parse_pr_body(1, "Just a regular PR body without preview");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_merge_base_from_diff_url() {
        let url = "https://whatpr.org/html/11741/74cbe0a...7ceff82/form-elements.html";
        assert_eq!(
            extract_merge_base_from_diff_url(url),
            Some("74cbe0a".to_string())
        );
    }
}
