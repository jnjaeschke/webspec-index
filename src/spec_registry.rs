use std::fmt::Write;

const AUTO_SPEC_PREFIX: &str = "AUTOURL-";

/// URL resolver and lightweight name/base-url inference.
pub struct SpecRegistry;

impl SpecRegistry {
    pub fn new() -> Self {
        Self
    }

    /// Map a URL to (derived_spec_name, anchor) if recognized.
    pub fn resolve_url(&self, url: &str) -> Option<(String, String)> {
        let (base_url, anchor) = auto_base_url_from_url(url)?;
        let spec_name = derive_spec_name_for_base_url(&base_url)
            .unwrap_or_else(|| auto_spec_name_for_base_url(&base_url));
        Some((spec_name, anchor))
    }

    /// Infer (base_url, provider) from a short spec name when possible.
    ///
    /// This intentionally only handles a narrow, low-ambiguity set:
    /// - previously auto-generated `AUTOURL-*` ids
    /// - `ECMA-262` -> tc39
    /// - generic WHATWG-style names (`HTML`, `DOM`, `URL`, ...)
    pub fn infer_base_url_from_spec_name(&self, spec_name: &str) -> Option<(String, String)> {
        if let Some(base_url) = auto_spec_base_url(spec_name) {
            return Some((
                base_url.clone(),
                provider_for_base_url(&base_url).to_string(),
            ));
        }

        let token = normalize_spec_token(spec_name);
        if token.is_empty() {
            return None;
        }

        if token == "ECMA-262" || token == "ECMA262" {
            return Some(("https://tc39.es/ecma262".to_string(), "tc39".to_string()));
        }

        let host_part = token.replace('-', "").to_ascii_lowercase();
        if host_part.is_empty() {
            return None;
        }

        Some((
            format!("https://{host_part}.spec.whatwg.org"),
            "whatwg".to_string(),
        ))
    }
}

impl Default for SpecRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_spec_token(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;

    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_uppercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

fn derive_spec_name_for_base_url(base_url: &str) -> Option<String> {
    let parsed = url::Url::parse(base_url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();

    let name = if host.ends_with(".spec.whatwg.org") {
        let slug = host.strip_suffix(".spec.whatwg.org")?;
        normalize_spec_token(slug)
    } else if host == "drafts.csswg.org"
        || host == "w3c.github.io"
        || host == "wicg.github.io"
        || host == "webaudio.github.io"
        || host == "tc39.es"
    {
        let first = parsed.path_segments()?.find(|seg| !seg.is_empty())?;
        if host == "tc39.es" && first.eq_ignore_ascii_case("ecma262") {
            "ECMA-262".to_string()
        } else {
            normalize_spec_token(first)
        }
    } else if host == "w3.org" || host == "www.w3.org" {
        let mut segs = parsed.path_segments()?;
        let first = segs.next()?;
        if first != "TR" {
            return None;
        }
        let second = segs.find(|seg| !seg.is_empty())?;
        normalize_spec_token(second)
    } else {
        return None;
    };

    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn auto_base_url_from_url(url: &str) -> Option<(String, String)> {
    let parsed = url::Url::parse(url).ok()?;
    let anchor = parsed.fragment()?.trim().to_string();
    if anchor.is_empty() {
        return None;
    }

    let scheme = parsed.scheme();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();

    let base_url = if host.ends_with(".spec.whatwg.org") {
        format!("{scheme}://{host}")
    } else if matches!(
        host.as_str(),
        "drafts.csswg.org" | "w3c.github.io" | "wicg.github.io" | "webaudio.github.io" | "tc39.es"
    ) {
        let first = parsed.path_segments()?.find(|seg| !seg.is_empty())?;
        format!("{scheme}://{host}/{first}")
    } else if host == "w3.org" || host == "www.w3.org" {
        let mut segs = parsed.path_segments()?;
        let first = segs.next()?;
        if first != "TR" {
            return None;
        }
        let second = segs.find(|seg| !seg.is_empty())?;
        format!("{scheme}://{host}/TR/{second}")
    } else {
        return None;
    };

    Some((base_url, anchor))
}

pub fn provider_for_base_url(base_url: &str) -> &'static str {
    let host = match url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    {
        Some(host) => host.to_ascii_lowercase(),
        None => return "dynamic",
    };

    if host.ends_with(".spec.whatwg.org") {
        return "whatwg";
    }
    if host == "tc39.es" {
        return "tc39";
    }
    if matches!(
        host.as_str(),
        "drafts.csswg.org"
            | "w3c.github.io"
            | "wicg.github.io"
            | "webaudio.github.io"
            | "w3.org"
            | "www.w3.org"
    ) {
        return "w3c";
    }
    "dynamic"
}

pub fn auto_spec_name_for_base_url(base_url: &str) -> String {
    let mut hex = String::with_capacity(base_url.len() * 2);
    for byte in base_url.as_bytes() {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("{AUTO_SPEC_PREFIX}{hex}")
}

pub fn auto_spec_base_url(spec_name: &str) -> Option<String> {
    let hex = spec_name.strip_prefix(AUTO_SPEC_PREFIX)?;
    if hex.len() % 2 != 0 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let chars: Vec<char> = hex.chars().collect();
    for pair in chars.chunks(2) {
        let hi = pair[0].to_digit(16)?;
        let lo = pair[1].to_digit(16)?;
        bytes.push(((hi << 4) | lo) as u8);
    }
    String::from_utf8(bytes).ok()
}

pub fn resolve_auto_url(url: &str) -> Option<(String, String)> {
    let (base_url, anchor) = auto_base_url_from_url(url)?;
    Some((auto_spec_name_for_base_url(&base_url), anchor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_spec_roundtrip() {
        let base_url = "https://wicg.github.io/permissions-policy";
        let spec = auto_spec_name_for_base_url(base_url);
        assert!(spec.starts_with(AUTO_SPEC_PREFIX));
        assert_eq!(auto_spec_base_url(&spec).as_deref(), Some(base_url));
    }

    #[test]
    fn resolve_url_derives_html_name() {
        let registry = SpecRegistry::new();
        let (spec, anchor) = registry
            .resolve_url("https://html.spec.whatwg.org/#navigate")
            .unwrap();
        assert_eq!(spec, "HTML");
        assert_eq!(anchor, "navigate");
    }

    #[test]
    fn resolve_auto_url_unknown_domain_rejected() {
        let registry = SpecRegistry::new();
        assert!(registry.resolve_url("https://example.com/#foo").is_none());
    }

    #[test]
    fn provider_inference() {
        assert_eq!(
            provider_for_base_url("https://html.spec.whatwg.org"),
            "whatwg"
        );
        assert_eq!(provider_for_base_url("https://tc39.es/ecma262"), "tc39");
        assert_eq!(
            provider_for_base_url("https://w3c.github.io/ServiceWorker"),
            "w3c"
        );
    }

    #[test]
    fn infer_base_url_from_short_name() {
        let registry = SpecRegistry::new();
        let (base, provider) = registry.infer_base_url_from_spec_name("HTML").unwrap();
        assert_eq!(base, "https://html.spec.whatwg.org");
        assert_eq!(provider, "whatwg");

        let (base, provider) = registry.infer_base_url_from_spec_name("ECMA-262").unwrap();
        assert_eq!(base, "https://tc39.es/ecma262");
        assert_eq!(provider, "tc39");
    }
}
