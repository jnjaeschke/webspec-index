use anyhow::{Context, Result};
use crate::db::{queries, write};
use crate::model::ParsedSpec;
use crate::parse;
use rusqlite::Connection;

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

/// Merge multiple ParsedSpec results (from multi-page fetches) into one.
pub fn merge_parsed_specs(specs: Vec<ParsedSpec>) -> ParsedSpec {
    let mut sections = Vec::new();
    let mut references = Vec::new();
    let mut idl_definitions = Vec::new();
    for spec in specs {
        sections.extend(spec.sections);
        references.extend(spec.references);
        idl_definitions.extend(spec.idl_definitions);
    }
    ParsedSpec { sections, references, idl_definitions }
}

/// Resolve a short SHA to a full SHA via GitHub API.
pub async fn resolve_full_sha(repo: &str, short_sha: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/commits/{short_sha}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", concat!("webspec-index/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?;
    let json: serde_json::Value = resp.json().await?;
    json["sha"]
        .as_str()
        .map(|s| s.to_string())
        .context("GitHub API response missing sha field")
}

/// Fetch the PR body from GitHub API and parse preview metadata.
pub async fn fetch_pr_preview(repo: &str, pr_number: i64) -> Result<PrPreview> {
    let url = format!("https://api.github.com/repos/{repo}/pulls/{pr_number}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", concat!("webspec-index/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context(format!("Failed to fetch PR #{pr_number} from {repo}"))?;
    let json: serde_json::Value = resp.json().await?;
    let body = json["body"]
        .as_str()
        .context("PR has no body")?;
    parse_pr_body(pr_number, body)
}

/// Fetch all preview pages from whatpr.org for a PR and parse them.
async fn fetch_pr_pages(preview: &PrPreview, spec_name: &str, base_url: &str) -> Result<ParsedSpec> {
    let mut parsed_pages = Vec::new();
    for page in &preview.pages {
        eprintln!("Fetching PR #{} page: {}", preview.pr_number, page.page_path);
        let html = super::fetch_raw_html(&page.url).await?;
        let parsed = parse::parse_spec(&html, spec_name, base_url)?;
        parsed_pages.push(parsed);
    }
    Ok(merge_parsed_specs(parsed_pages))
}

/// Fetch the merge base spec from WHATWG commit snapshots.
async fn fetch_merge_base(
    spec_name: &str,
    base_url: &str,
    full_sha: &str,
) -> Result<ParsedSpec> {
    let host = base_url
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let url = format!("https://{host}/commit-snapshots/{full_sha}/");
    eprintln!("Fetching merge base {}: {}", spec_name, &url[..url.len().min(80)]);
    let html = super::fetch_raw_html(&url).await?;
    parse::parse_spec(&html, spec_name, base_url)
}

/// Ensure a PR snapshot is indexed and fresh.
///
/// Returns (pr_snapshot_id, merge_base_snapshot_id).
/// If the PR is already indexed with the same head SHA, returns cached IDs.
pub async fn ensure_pr_indexed(
    conn: &Connection,
    spec_name: &str,
    base_url: &str,
    provider: &str,
    pr_number: i64,
) -> Result<(i64, i64)> {
    let spec_id = write::insert_or_get_spec(conn, spec_name, base_url, provider)?;

    // Determine the WHATWG repo name from spec name
    let repo = format!("whatwg/{}", spec_name.to_lowercase());

    // Fetch PR preview metadata from GitHub
    let preview = fetch_pr_preview(&repo, pr_number).await?;

    // Check if we already have this PR indexed with the same head SHA
    if let Some((pr_snap_id, stored_base_sha)) = queries::get_pr_snapshot(conn, spec_name, pr_number)? {
        let pr_sha: String = conn.query_row(
            "SELECT sha FROM snapshots WHERE id = ?1",
            [pr_snap_id],
            |row| row.get(0),
        )?;
        if pr_sha.ends_with(&preview.head_sha) {
            // Still fresh — find the merge base snapshot
            if let Some(base_snap_id) = queries::get_commit_snapshot(conn, spec_id, &stored_base_sha)? {
                return Ok((pr_snap_id, base_snap_id));
            }
        }
        // Stale — delete old PR data
        write::delete_pr_data(conn, spec_id, pr_number)?;
    }

    // Resolve short merge base SHA to full SHA
    let full_base_sha = resolve_full_sha(&repo, &preview.merge_base_sha).await?;

    // Fetch or reuse merge base snapshot
    let base_snap_id = if let Some(id) = queries::get_commit_snapshot(conn, spec_id, &full_base_sha)? {
        id
    } else {
        let base_parsed = fetch_merge_base(spec_name, base_url, &full_base_sha).await?;
        let commit_date = chrono::Utc::now().to_rfc3339();
        let id = write::insert_snapshot(conn, spec_id, &full_base_sha, &commit_date)?;
        write::insert_sections_bulk(conn, id, &base_parsed.sections)?;
        write::insert_refs_bulk(conn, id, &base_parsed.references)?;
        write::insert_idl_defs_bulk(conn, id, &base_parsed.idl_definitions)?;
        id
    };

    // Fetch and parse PR pages
    let pr_parsed = fetch_pr_pages(&preview, spec_name, base_url).await?;
    let pr_sha = format!("pr:{}:{}", pr_number, preview.head_sha);
    let commit_date = chrono::Utc::now().to_rfc3339();
    let pr_snap_id = write::insert_pr_snapshot(
        conn, spec_id, &pr_sha, &commit_date, pr_number, &full_base_sha,
    )?;
    write::insert_sections_bulk(conn, pr_snap_id, &pr_parsed.sections)?;
    write::insert_refs_bulk(conn, pr_snap_id, &pr_parsed.references)?;
    write::insert_idl_defs_bulk(conn, pr_snap_id, &pr_parsed.idl_definitions)?;

    Ok((pr_snap_id, base_snap_id))
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

    #[test]
    fn test_merge_parsed_specs() {
        use crate::model::{ParsedSpec, ParsedSection, ParsedReference, SectionType};

        let spec1 = ParsedSpec {
            sections: vec![ParsedSection {
                anchor: "sec-a".into(), title: Some("A".into()), content_text: None,
                section_type: SectionType::Heading, parent_anchor: None,
                prev_anchor: None, next_anchor: None, depth: Some(2),
            }],
            references: vec![],
            idl_definitions: vec![],
        };
        let spec2 = ParsedSpec {
            sections: vec![ParsedSection {
                anchor: "sec-b".into(), title: Some("B".into()), content_text: None,
                section_type: SectionType::Heading, parent_anchor: None,
                prev_anchor: None, next_anchor: None, depth: Some(2),
            }],
            references: vec![ParsedReference {
                from_anchor: "sec-b".into(), to_spec: "DOM".into(), to_anchor: "concept-tree".into(),
            }],
            idl_definitions: vec![],
        };

        let merged = merge_parsed_specs(vec![spec1, spec2]);
        assert_eq!(merged.sections.len(), 2);
        assert_eq!(merged.references.len(), 1);
    }
}
