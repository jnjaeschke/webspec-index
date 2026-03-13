//! LSP server for spec-aware editor integration.
//!
//! Provides hover on spec URLs, step comment validation with diagnostics,
//! inlay hints, code lens coverage, and debounced document analysis.

mod hover;
mod server;

pub use server::serve_stdio;
