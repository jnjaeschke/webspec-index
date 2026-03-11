use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use std::collections::HashSet;

pub struct IETFProvider;

const DATATRACKER_API_BASE: &str = "https://datatracker.ietf.org/api/v1/doc";
const USER_AGENT: &str = concat!("webspec-index/", env!("CARGO_PKG_VERSION"));

// ── Name parsing ─────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum IetfDocKind {
    Rfc,
    Draft,
}

pub struct ParsedIetfName<'a> {
    pub kind: IetfDocKind,
    /// Base name (no version suffix): "rfc9187" or "draft-touch-sne"
    pub base: &'a str,
    /// Pinned revision for drafts only: Some("02")
    pub pinned_rev: Option<&'a str>,
}

/// Parse an IETF document name into its components.
///
/// Examples:
/// - "RFC9110"          → Rfc, "RFC9110", None
/// - "rfc9110"          → Rfc, "rfc9110", None
/// - "draft-touch-sne"  → Draft, "draft-touch-sne", None   (latest)
/// - "draft-touch-sne-02" → Draft, "draft-touch-sne", Some("02")  (pinned)
pub fn parse_ietf_name(name: &str) -> ParsedIetfName<'_> {
    // RFC: first three characters are "rfc" (case-insensitive)
    if name.len() >= 3 && name[..3].eq_ignore_ascii_case("rfc") {
        return ParsedIetfName {
            kind: IetfDocKind::Rfc,
            base: name,
            pinned_rev: None,
        };
    }

    // Draft: check for trailing -NN version suffix (exactly 2 ASCII digits)
    if name.len() > 3 {
        let suffix = &name[name.len() - 2..];
        let sep = name.as_bytes().get(name.len() - 3).copied();
        if sep == Some(b'-') && suffix.bytes().all(|b| b.is_ascii_digit()) {
            return ParsedIetfName {
                kind: IetfDocKind::Draft,
                base: &name[..name.len() - 3],
                pinned_rev: Some(suffix),
            };
        }
    }

    ParsedIetfName {
        kind: IetfDocKind::Draft,
        base: name,
        pinned_rev: None,
    }
}

/// Convert any IETF document name to its canonical display form.
/// - RFC names become uppercase: "rfc9187" → "RFC9187"
/// - Draft names have their version suffix stripped: "draft-touch-sne-02" → "draft-touch-sne"
pub fn canonical_ietf_name(name: &str) -> String {
    let parsed = parse_ietf_name(name);
    match parsed.kind {
        IetfDocKind::Rfc => {
            // Uppercase RFC + number, e.g. "RFC9187"
            let lower = parsed.base.to_lowercase();
            if let Some(num_str) = lower.strip_prefix("rfc") {
                if let Ok(num) = num_str.parse::<u32>() {
                    return format!("RFC{}", num);
                }
            }
            parsed.base.to_uppercase()
        }
        IetfDocKind::Draft => parsed.base.to_string(),
    }
}

// ── Datatracker API types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DataTrackerDoc {
    rev: String,
    #[allow(dead_code)]
    time: String,
    #[serde(default)]
    expires: Option<String>,
}

#[derive(Deserialize)]
struct RelatedDocsResponse {
    objects: Vec<RelatedDoc>,
}

#[derive(Deserialize)]
struct RelatedDoc {
    source: String,
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

fn make_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(Into::into)
}

/// Fetch metadata for an IETF document from the Datatracker API.
/// Returns Ok(None) if the document is not found (404).
async fn fetch_doc_meta(client: &reqwest::Client, name: &str) -> Result<Option<DataTrackerDoc>> {
    let url = format!("{}/document/{}/", DATATRACKER_API_BASE, name.to_lowercase());
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        anyhow::bail!(
            "Datatracker API error for '{}': HTTP {}",
            name,
            resp.status()
        );
    }

    Ok(Some(resp.json::<DataTrackerDoc>().await?))
}

/// Extract a bare document name from a Datatracker resource URI.
/// e.g. "/api/v1/doc/document/rfc9110/" → "rfc9110"
fn extract_doc_name_from_uri(uri: &str) -> Option<&str> {
    uri.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
}

/// Follow the obsoleted_by chain for an RFC until we reach the terminal
/// (non-obsoleted) RFC. Returns the terminal RFC name (lowercase).
///
/// If multiple RFCs obsolete the same RFC (rare), picks the one with the
/// highest RFC number (i.e. the most recently published).
async fn follow_obsoleted_by_chain(client: &reqwest::Client, start_name: &str) -> Result<String> {
    let mut current = start_name.to_lowercase();
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        if !seen.insert(current.clone()) {
            // Cycle guard (should not happen in practice)
            break;
        }

        // Find documents that obsolete `current` (i.e., relationship=obs, target=current)
        let url = format!(
            "{}/relateddocument/?target__name={}&relationship=obs&format=json",
            DATATRACKER_API_BASE, current
        );
        let resp = client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            break;
        }

        let data: RelatedDocsResponse = resp.json().await?;
        if data.objects.is_empty() {
            break; // Nothing obsoletes `current` → it's the latest
        }

        // Pick the obsoleting RFC with the highest number
        let mut best_num: Option<u32> = None;
        let mut best_name = String::new();
        for obj in &data.objects {
            if let Some(doc_name) = extract_doc_name_from_uri(&obj.source) {
                if let Some(num_str) = doc_name.strip_prefix("rfc") {
                    if let Ok(num) = num_str.parse::<u32>() {
                        if best_num.is_none() || num > best_num.unwrap() {
                            best_num = Some(num);
                            best_name = doc_name.to_string();
                        }
                    }
                }
            }
        }

        if best_name.is_empty() {
            break;
        }
        current = best_name;
    }

    Ok(current)
}

/// Check if a draft document is expired based on its expires field.
fn is_expired(doc: &DataTrackerDoc) -> bool {
    let Some(ref expires_str) = doc.expires else {
        return false;
    };
    parse_ietf_date(expires_str)
        .map(|expires| expires < Utc::now())
        .unwrap_or(false)
}

/// Parse an IETF date string which may be RFC3339, "YYYY-MM-DDTHH:MM:SS" (no tz),
/// or plain "YYYY-MM-DD".
fn parse_ietf_date(s: &str) -> Result<DateTime<Utc>> {
    // Try RFC3339
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Try with appended 'Z'
    if let Ok(dt) = DateTime::parse_from_rfc3339(&format!("{}Z", s)) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Try plain date
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(d.and_hms_opt(0, 0, 0).unwrap().and_utc());
    }
    anyhow::bail!("Cannot parse IETF date: '{}'", s)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Dynamically resolve an IETF RFC or draft name to (canonical_name, base_url, "ietf").
///
/// - "RFC9110" or "rfc9110"   → follows obsoleted_by chain to the latest RFC
/// - "draft-touch-sne"        → resolves to the latest revision
/// - "draft-touch-sne-02"     → pinned to revision 02
///
/// Returns Ok(None) if the document cannot be found in the Datatracker.
/// Returns Ok(None) if the name is not an RFC or draft name.
pub async fn discover_ietf_spec(name: &str) -> Result<Option<(String, String, String)>> {
    let parsed = parse_ietf_name(name);

    // Only handle names that look like RFC or draft
    match parsed.kind {
        IetfDocKind::Rfc => {}
        IetfDocKind::Draft => {
            if !name.starts_with("draft-") && !name.starts_with("Draft-") {
                return Ok(None);
            }
        }
    }

    let client = make_http_client()?;

    match parsed.kind {
        IetfDocKind::Rfc => {
            let rfc_base = parsed.base.to_lowercase();
            // Verify it exists
            if fetch_doc_meta(&client, &rfc_base).await?.is_none() {
                return Ok(None);
            }
            // Follow the obsoleted_by chain to the terminal RFC
            let terminal = follow_obsoleted_by_chain(&client, &rfc_base).await?;
            let lower = terminal.to_lowercase();
            let num_str = lower.strip_prefix("rfc").unwrap_or(&lower);
            let num: u32 = num_str.parse().unwrap_or(0);
            let canonical = format!("RFC{}", num);
            let base_url = format!("https://www.rfc-editor.org/rfc/rfc{}.html", num);
            Ok(Some((canonical, base_url, "ietf".to_string())))
        }

        IetfDocKind::Draft => {
            let base = parsed.base;
            let doc = match fetch_doc_meta(&client, base).await? {
                Some(d) => d,
                None => return Ok(None),
            };

            if is_expired(&doc) {
                eprintln!("Warning: IETF draft '{}' is expired or inactive", base);
            }

            let rev = if let Some(pinned) = parsed.pinned_rev {
                pinned.to_string()
            } else {
                doc.rev.clone()
            };

            // Pinned-version drafts get a versioned DB name so they are stored
            // separately from the unversioned (latest-tracking) entry.
            let canonical_name = if parsed.pinned_rev.is_some() {
                format!("{}-{}", base, rev)
            } else {
                base.to_string()
            };

            let base_url = format!("https://datatracker.ietf.org/doc/html/{}-{}", base, rev);

            Ok(Some((canonical_name, base_url, "ietf".to_string())))
        }
    }
}

/// Map an IETF-related URL to (canonical_spec_name, anchor).
///
/// Recognised URL patterns:
/// - `datatracker.ietf.org/doc/html/rfc{N}#{frag}`
/// - `datatracker.ietf.org/doc/html/draft-name-{NN}#{frag}`
/// - `datatracker.ietf.org/doc/rfc{N}/#{frag}`
/// - `www.rfc-editor.org/rfc/rfc{N}#{frag}`
/// - `www.rfc-editor.org/rfc/rfc{N}.html#{frag}`
/// - `www.ietf.org/archive/id/draft-name-{NN}.html#{frag}`
/// - `www.ietf.org/archive/id/draft-name-{NN}.txt#{frag}`
pub fn resolve_ietf_url(url: &str) -> Option<(String, String)> {
    let parsed = url::Url::parse(url).ok()?;
    let fragment = parsed.fragment()?.to_string();
    let host = parsed.host_str()?;
    let path = parsed.path();

    match host {
        "datatracker.ietf.org" => {
            let path = path.trim_matches('/');
            if let Some(rest) = path.strip_prefix("doc/html/") {
                return Some((canonical_ietf_name(rest), fragment));
            }
            if let Some(rest) = path.strip_prefix("doc/") {
                let doc_name = rest.trim_end_matches('/');
                if doc_name.starts_with("rfc") || doc_name.starts_with("draft-") {
                    return Some((canonical_ietf_name(doc_name), fragment));
                }
            }
            None
        }

        "www.rfc-editor.org" => {
            let path = path.trim_matches('/');
            if let Some(rest) = path.strip_prefix("rfc/") {
                let doc_name = rest.trim_end_matches(".html");
                if doc_name.starts_with("rfc") {
                    return Some((canonical_ietf_name(doc_name), fragment));
                }
            }
            None
        }

        "www.ietf.org" => {
            let path = path.trim_matches('/');
            if let Some(rest) = path.strip_prefix("archive/id/") {
                let doc_name = rest.trim_end_matches(".html").trim_end_matches(".txt");
                if doc_name.starts_with("draft-") {
                    return Some((canonical_ietf_name(doc_name), fragment));
                }
            }
            None
        }

        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_ietf_name ──────────────────────────────────────────────────────

    #[test]
    fn test_parse_rfc_uppercase() {
        let p = parse_ietf_name("RFC9110");
        assert!(matches!(p.kind, IetfDocKind::Rfc));
        assert_eq!(p.base, "RFC9110");
        assert!(p.pinned_rev.is_none());
    }

    #[test]
    fn test_parse_rfc_lowercase() {
        let p = parse_ietf_name("rfc9110");
        assert!(matches!(p.kind, IetfDocKind::Rfc));
        assert_eq!(p.base, "rfc9110");
        assert!(p.pinned_rev.is_none());
    }

    #[test]
    fn test_parse_draft_latest() {
        let p = parse_ietf_name("draft-touch-sne");
        assert!(matches!(p.kind, IetfDocKind::Draft));
        assert_eq!(p.base, "draft-touch-sne");
        assert!(p.pinned_rev.is_none());
    }

    #[test]
    fn test_parse_draft_pinned() {
        let p = parse_ietf_name("draft-touch-sne-02");
        assert!(matches!(p.kind, IetfDocKind::Draft));
        assert_eq!(p.base, "draft-touch-sne");
        assert_eq!(p.pinned_rev, Some("02"));
    }

    #[test]
    fn test_parse_draft_long_name_pinned() {
        // draft-ietf-dtn-bpsec-default-sc-11
        let p = parse_ietf_name("draft-ietf-dtn-bpsec-default-sc-11");
        assert!(matches!(p.kind, IetfDocKind::Draft));
        assert_eq!(p.base, "draft-ietf-dtn-bpsec-default-sc");
        assert_eq!(p.pinned_rev, Some("11"));
    }

    #[test]
    fn test_parse_draft_active_no_version() {
        let p = parse_ietf_name("draft-hancke-webrtc-sped");
        assert!(matches!(p.kind, IetfDocKind::Draft));
        assert_eq!(p.base, "draft-hancke-webrtc-sped");
        assert!(p.pinned_rev.is_none());
    }

    // ── canonical_ietf_name ──────────────────────────────────────────────────

    #[test]
    fn test_canonical_rfc_lower() {
        assert_eq!(canonical_ietf_name("rfc9187"), "RFC9187");
    }

    #[test]
    fn test_canonical_rfc_upper() {
        assert_eq!(canonical_ietf_name("RFC9187"), "RFC9187");
    }

    #[test]
    fn test_canonical_draft_latest() {
        assert_eq!(canonical_ietf_name("draft-touch-sne"), "draft-touch-sne");
    }

    #[test]
    fn test_canonical_draft_versioned() {
        assert_eq!(canonical_ietf_name("draft-touch-sne-02"), "draft-touch-sne");
    }

    // ── resolve_ietf_url ─────────────────────────────────────────────────────

    #[test]
    fn test_resolve_datatracker_rfc_html() {
        let r = resolve_ietf_url("https://datatracker.ietf.org/doc/html/rfc9110#section-5");
        assert_eq!(r, Some(("RFC9110".to_string(), "section-5".to_string())));
    }

    #[test]
    fn test_resolve_datatracker_draft_html() {
        let r =
            resolve_ietf_url("https://datatracker.ietf.org/doc/html/draft-touch-sne-02#section-1");
        // Version suffix stripped → canonical base name
        assert_eq!(
            r,
            Some(("draft-touch-sne".to_string(), "section-1".to_string()))
        );
    }

    #[test]
    fn test_resolve_datatracker_doc_rfc() {
        let r = resolve_ietf_url("https://datatracker.ietf.org/doc/rfc9187/#section-2");
        assert_eq!(r, Some(("RFC9187".to_string(), "section-2".to_string())));
    }

    #[test]
    fn test_resolve_rfc_editor_bare() {
        let r = resolve_ietf_url("https://www.rfc-editor.org/rfc/rfc9110#section-5");
        assert_eq!(r, Some(("RFC9110".to_string(), "section-5".to_string())));
    }

    #[test]
    fn test_resolve_rfc_editor_html_ext() {
        let r = resolve_ietf_url("https://www.rfc-editor.org/rfc/rfc9110.html#section-5");
        assert_eq!(r, Some(("RFC9110".to_string(), "section-5".to_string())));
    }

    #[test]
    fn test_resolve_ietf_archive_html() {
        let r =
            resolve_ietf_url("https://www.ietf.org/archive/id/draft-touch-sne-02.html#section-1");
        assert_eq!(
            r,
            Some(("draft-touch-sne".to_string(), "section-1".to_string()))
        );
    }

    #[test]
    fn test_resolve_ietf_archive_txt() {
        let r = resolve_ietf_url(
            "https://www.ietf.org/archive/id/draft-ietf-dtn-bpsec-default-sc-11.txt#section-3",
        );
        assert_eq!(
            r,
            Some((
                "draft-ietf-dtn-bpsec-default-sc".to_string(),
                "section-3".to_string()
            ))
        );
    }

    #[test]
    fn test_resolve_no_fragment() {
        assert_eq!(
            resolve_ietf_url("https://www.rfc-editor.org/rfc/rfc9110.html"),
            None
        );
    }

    #[test]
    fn test_resolve_unknown_host() {
        assert_eq!(
            resolve_ietf_url("https://example.com/rfc/rfc9110#foo"),
            None
        );
        assert_eq!(
            resolve_ietf_url("https://html.spec.whatwg.org/#navigate"),
            None
        );
    }
}
