//! Shared possible-effects analysis for specification queries and editors.
pub mod bundled;
pub mod catalog;
#[cfg_attr(not(any(feature = "native", test)), allow(dead_code))]
pub(crate) mod compact;
pub mod engine;
pub mod fragment;
pub mod graph;
pub mod link;
pub mod matcher;
pub mod model;
pub mod query;
pub mod render;
pub mod service;

pub use bundled::default_catalog;
#[cfg(feature = "native")]
pub use catalog::load_package;
pub use catalog::{
    load_catalog, load_catalog_sources, load_package_files, Catalog, CatalogError, Package,
};
pub use engine::{
    analyze, AnalysisArtifact, AnalysisInput, ArtifactSummary, IndexedAnchor, SourceSpec,
};
pub use model::*;
#[cfg(not(feature = "native"))]
pub use query::QueryWithEffects;
#[cfg(feature = "native")]
pub use query::{query_section_with_effects, QueryWithEffects};
#[cfg(feature = "native")]
pub use service::{
    explain_effects, get_effect_preview, get_effect_summary, input_fingerprint, rebuild_effects,
    recompute_effects,
};
