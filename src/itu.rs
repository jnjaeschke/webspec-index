// ITU-T Recommendation support
//
// Handles name parsing/canonicalization, dynamic edition discovery via the
// rec.aspx listing page, and canonical free-PDF URL construction.

/// Check whether a spec name looks like an ITU-T Recommendation: one letter,
/// a dot, then one or more dot-separated digit groups (e.g. "H.265",
/// "H.265.1", "T.35", "G.711"). Case-insensitive on the leading letter.
pub fn is_itu_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    if bytes.get(1) != Some(&b'.') {
        return false;
    }
    let rest = &name[2..];
    if rest.is_empty() {
        return false;
    }
    rest.split('.')
        .all(|group| !group.is_empty() && group.bytes().all(|b| b.is_ascii_digit()))
}

/// Convert an ITU-T Recommendation name to its canonical form: uppercase
/// leading letter, rest unchanged (it's already digits and dots).
/// "h.265" -> "H.265", "H.265" -> "H.265".
pub fn canonical_itu_name(name: &str) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!(
        "{}{}",
        first.to_ascii_uppercase(),
        &name[first.len_utf8()..]
    )
}

use anyhow::Result;
use regex::Regex;

const USER_AGENT: &str = concat!("webspec-index/", env!("CARGO_PKG_VERSION"));
const REC_ASPX_BASE: &str = "https://www.itu.int/ITU-T/recommendations/rec.aspx";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Edition {
    date: String,   // "YYYYMM" — lexically sortable
    status: String, // "I" (in force), "S" (superseded), "P" (pre-published), ...
}

fn edition_pattern(name: &str) -> Result<Regex> {
    let escaped = regex::escape(name);
    let pattern = format!(r"T-REC-{escaped}-(\d{{6}})-([A-Za-z]+)");
    Regex::new(&pattern)
        .map_err(|e| anyhow::anyhow!("Invalid ITU-T edition regex for '{}': {}", name, e))
}

/// Parse every `T-REC-<name>-<YYYYMM>-<STATUS>` occurrence out of a rec.aspx
/// listing page. `name` must already be canonicalized (uppercase).
fn parse_editions(html: &str, name: &str) -> Result<Vec<Edition>> {
    let re = edition_pattern(name)?;
    let mut seen = std::collections::HashSet::new();
    let mut editions = Vec::new();
    for cap in re.captures_iter(html) {
        let date = cap[1].to_string();
        let status = cap[2].to_ascii_uppercase();
        if seen.insert((date.clone(), status.clone())) {
            editions.push(Edition { date, status });
        }
    }
    Ok(editions)
}

/// Pick the edition to serve: the latest (max date) with status "I" (in
/// force). If none is "I", fall back to the latest edition overall and warn
/// — serving a superseded/pre-published text without saying so would
/// silently mislead a reviewer citing it.
fn select_edition(editions: &[Edition]) -> &Edition {
    if let Some(best) = editions
        .iter()
        .filter(|e| e.status == "I")
        .max_by(|a, b| a.date.cmp(&b.date))
    {
        return best;
    }

    let latest = editions
        .iter()
        .max_by(|a, b| a.date.cmp(&b.date))
        .expect("select_edition called with empty editions");
    eprintln!(
        "Warning: no in-force (status I) edition found; using latest edition dated {} (status {})",
        latest.date, latest.status
    );
    latest
}

fn pdf_url_for(name: &str, edition: &Edition) -> String {
    format!(
        "https://www.itu.int/rec/dologin_pub.asp?lang=e&id=T-REC-{}-{}-{}!!PDF-E&type=items",
        name, edition.date, edition.status
    )
}

fn make_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(Into::into)
}

/// Dynamically resolve an ITU-T Recommendation name to its latest in-force
/// edition's free PDF URL, by scraping the rec.aspx listing page (there is
/// no API). Returns `Ok(None)` if the name isn't a recognized
/// recommendation (no editions found on the page).
///
/// Note: itu.int has announced this page will eventually be replaced; if the
/// scrape starts returning zero matches for known-good names, that's the
/// likely cause.
pub async fn discover_spec(name: &str) -> Result<Option<(String, String)>> {
    let canonical = canonical_itu_name(name);
    let client = make_http_client()?;
    let url = format!("{}?rec={}", REC_ASPX_BASE, canonical);
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let html = resp.text().await?;

    let editions = parse_editions(&html, &canonical)?;
    if editions.is_empty() {
        return Ok(None);
    }

    let chosen = select_edition(&editions);
    let pdf_url = pdf_url_for(&canonical, chosen);
    Ok(Some((canonical, pdf_url)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_itu_name_basic() {
        assert!(is_itu_name("H.265"));
        assert!(is_itu_name("h.265"));
        assert!(is_itu_name("T.35"));
        assert!(is_itu_name("G.711"));
    }

    #[test]
    fn test_is_itu_name_multi_part() {
        assert!(is_itu_name("H.265.1"));
    }

    #[test]
    fn test_is_itu_name_rejects_non_itu() {
        assert!(!is_itu_name("HTML"));
        assert!(!is_itu_name("RFC9110"));
        assert!(!is_itu_name("CSS-GRID"));
        assert!(!is_itu_name("H."));
        assert!(!is_itu_name("H.26A"));
        assert!(!is_itu_name(""));
        assert!(!is_itu_name("H"));
    }

    #[test]
    fn test_canonical_itu_name() {
        assert_eq!(canonical_itu_name("h.265"), "H.265");
        assert_eq!(canonical_itu_name("H.265"), "H.265");
        assert_eq!(canonical_itu_name("t.35"), "T.35");
    }
}

#[cfg(test)]
mod discover_tests {
    use super::*;

    const FIXTURE_HTML: &str = r#"
        <table>
          <tr><td><a href="/rec/dologin_pub.asp?lang=e&amp;id=T-REC-H.265-201612-S!!PDF-E&amp;type=items">2016</a></td></tr>
          <tr><td><a href="/rec/dologin_pub.asp?lang=e&amp;id=T-REC-H.265-201802-I!!PDF-E&amp;type=items">2018</a></td></tr>
          <tr><td><a href="/rec/dologin_pub.asp?lang=e&amp;id=T-REC-H.265-201502-S!!PDF-E&amp;type=items">2015</a></td></tr>
        </table>
    "#;

    #[test]
    fn test_parse_editions_finds_all() {
        let editions = parse_editions(FIXTURE_HTML, "H.265").unwrap();
        assert_eq!(editions.len(), 3);
    }

    #[test]
    fn test_parse_editions_dedups() {
        let html = format!("{FIXTURE_HTML}{FIXTURE_HTML}");
        let editions = parse_editions(&html, "H.265").unwrap();
        assert_eq!(editions.len(), 3, "duplicate occurrences must be deduped");
    }

    #[test]
    fn test_parse_editions_no_match_returns_empty() {
        let editions = parse_editions("<html>nothing here</html>", "H.265").unwrap();
        assert!(editions.is_empty());
    }

    #[test]
    fn test_select_edition_prefers_latest_in_force() {
        let editions = parse_editions(FIXTURE_HTML, "H.265").unwrap();
        let chosen = select_edition(&editions);
        assert_eq!(chosen.date, "201802");
        assert_eq!(chosen.status, "I");
    }

    #[test]
    fn test_select_edition_falls_back_when_no_in_force() {
        // H.264-style edge case: newest edition is pre-published ("P"), not
        // yet in force; the older "I" edition must still win.
        let editions = vec![
            Edition {
                date: "202001".to_string(),
                status: "I".to_string(),
            },
            Edition {
                date: "202405".to_string(),
                status: "P".to_string(),
            },
        ];
        let chosen = select_edition(&editions);
        assert_eq!(chosen.date, "202001");
        assert_eq!(chosen.status, "I");
    }

    #[test]
    fn test_select_edition_no_in_force_at_all() {
        let editions = vec![
            Edition {
                date: "202001".to_string(),
                status: "S".to_string(),
            },
            Edition {
                date: "202405".to_string(),
                status: "P".to_string(),
            },
        ];
        let chosen = select_edition(&editions);
        assert_eq!(
            chosen.date, "202405",
            "falls back to latest overall when no I edition exists"
        );
    }

    #[test]
    fn test_pdf_url_format() {
        let edition = Edition {
            date: "201802".to_string(),
            status: "I".to_string(),
        };
        let url = pdf_url_for("H.265", &edition);
        assert_eq!(
            url,
            "https://www.itu.int/rec/dologin_pub.asp?lang=e&id=T-REC-H.265-201802-I!!PDF-E&type=items"
        );
    }
}
