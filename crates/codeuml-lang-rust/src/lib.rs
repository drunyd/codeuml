//! Rust syntax analysis for CodeUML.
//!
//! The analyzer parses Rust files with `syn` and lowers declarations into the
//! language-neutral `codeuml-core` model. It also emits contextual
//! [`RustReference`] values for fields, methods, aliases, trait bounds, and
//! trait implementations. Project-wide resolution is performed by
//! `codeuml-analysis`; no compiler or rust-analyzer integration is attempted.
//!
//! Module traversal follows conventional Rust file layout:
//! `foo.rs`/`foo/mod.rs`, inline modules, crate roots, and nested module
//! directories. Files are only read when discovery supplied them.
//!
//! Syntax analysis does not require a successful build. Files are parsed
//! independently; a parse failure produces a structured diagnostic while other
//! files continue to contribute to the result.

mod analyzer;
mod input;
mod references;

pub use analyzer::analyze_package;
pub use input::{RustPackageSources, RustTargetSource};
pub use references::{RustImport, RustPathRoot, RustReference, RustTypePath};

use codeuml_core::{Diagnostic, Module, TypeDecl};

/// Syntax-only result for one Cargo package.
#[derive(Clone, Debug, Default)]
pub struct RustAnalysis {
    pub modules: Vec<Module>,
    pub types: Vec<TypeDecl>,
    pub references: Vec<RustReference>,
    pub diagnostics: Vec<Diagnostic>,
    pub files_analyzed: usize,
    pub files_failed: usize,
}
