//! Rust support for CodeUML.
//!
//! [`RustProvider`] implements `codeuml_lang::LanguageProvider`: it discovers
//! Cargo packages (`cargo metadata --no-deps`, no downloads or compilation),
//! parses each file with `syn`, lowers declarations into the language-neutral
//! `codeuml-core` model, and resolves the extracted [`RustReference`] values
//! against the merged project declaration index. No compiler or rust-analyzer
//! integration is attempted.
//!
//! Module traversal follows conventional Rust file layout:
//! `foo.rs`/`foo/mod.rs`, inline modules, crate roots, and nested module
//! directories. Files are only read when discovery supplied them.
//!
//! Syntax analysis does not require a successful build. Files are parsed
//! independently; a parse failure produces a structured diagnostic while other
//! files continue to contribute to the result.

mod analyzer;
mod discovery;
mod input;
mod provider;
mod references;
mod resolution;

pub use analyzer::analyze_package;
pub use discovery::{
    discover_project, DiscoveredPackage, DiscoveredProject, DiscoveredTarget, DiscoveryError,
};
pub use input::{RustPackageSources, RustTargetSource};
pub use provider::RustProvider;
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
