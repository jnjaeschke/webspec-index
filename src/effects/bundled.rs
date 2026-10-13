//! The maintained catalog is embedded in installed binaries, with no network lookup.
use super::catalog::{
    catalog_digest_of, load_catalog, load_package_files, package_digest, Catalog,
};
use super::model::{RequestError, RequestErrorCode};
use crate::semantics::bundled::FILES;
use std::sync::OnceLock;

/// Content digest of the bundled catalog, computed from the embedded files
/// without compiling their rules.
pub fn bundled_catalog_digest() -> Result<String, RequestError> {
    static DIGEST: OnceLock<Result<String, String>> = OnceLock::new();
    DIGEST
        .get_or_init(|| {
            let package = package_digest(FILES).map_err(|e| e.to_string())?;
            catalog_digest_of(&[package]).map_err(|e| e.to_string())
        })
        .clone()
        .map_err(invalid_catalog)
}

pub fn default_catalog(additional_paths: &[String]) -> Result<Catalog, RequestError> {
    static BUNDLED: OnceLock<Result<Catalog, String>> = OnceLock::new();
    let bundled = BUNDLED.get_or_init(|| {
        let package = load_package_files(FILES).map_err(|e| e.to_string())?;
        load_catalog([package]).map_err(|e| e.to_string())
    });
    let bundled = bundled
        .as_ref()
        .map_err(|message| invalid_catalog(message.clone()))?;
    if additional_paths.is_empty() {
        return Ok(bundled.clone());
    }
    #[cfg(not(feature = "native"))]
    {
        Err(invalid_catalog(format!(
            "catalog directory {} requires the native feature",
            additional_paths[0]
        )))
    }
    #[cfg(feature = "native")]
    {
        let mut packages = bundled.packages.clone();
        for path in additional_paths {
            packages.push(
                super::catalog::load_package(path).map_err(|e| invalid_catalog(e.to_string()))?,
            );
        }
        load_catalog(packages).map_err(|e| invalid_catalog(e.to_string()))
    }
}

fn invalid_catalog(message: String) -> RequestError {
    RequestError {
        code: RequestErrorCode::InvalidCatalog,
        message,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_digest_matches_the_loaded_catalog() {
        assert_eq!(
            bundled_catalog_digest().unwrap(),
            default_catalog(&[]).unwrap().content_digest
        );
    }

    #[test]
    fn bundled_catalog_loads() {
        let catalog = default_catalog(&[]).unwrap();
        assert_eq!(catalog.effects.len(), 8);
        assert!(catalog.rules().any(
            |rule| rule.emit.kind == "script.opportunity" && rule.match_spec.subject.is_some()
        ));
    }
}
