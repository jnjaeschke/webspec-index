//! Source code analysis for spec references and step comments.
//!
//! This module provides language-agnostic analysis of source files to find
//! spec URL references, validate step comments against spec algorithms,
//! and compute coverage metrics.
//!
//! Used by both the LSP server and the `analyze` CLI subcommand.

pub mod coverage;
pub mod file;
pub mod matcher;
pub mod scanner;
pub mod searchfox;
pub mod steps;
