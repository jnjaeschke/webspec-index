//! LSP server for spec-aware editor integration.
//!
//! Provides hover on spec URLs, step comment validation with diagnostics,
//! inlay hints, code lens coverage, and debounced document analysis.

mod coverage;
mod hover;
mod matcher;
mod scanner;
mod server;
mod steps;

pub use server::serve_stdio;
