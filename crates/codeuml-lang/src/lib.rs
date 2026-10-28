//! The contract between CodeUML and a source language.
//!
//! A [`LanguageProvider`] knows how to recognise a project written in one
//! language and how to turn its sources into the language-neutral
//! [`Codebase`] model: discovering files, extracting declarations, and
//! resolving references. Everything language-specific (project layout, parser,
//! import and name-resolution rules) lives behind this trait, so the model,
//! diagrams, and views never depend on a particular language.
//!
//! Providers fill a shared [`Codebase`] rather than returning their own, so a
//! project that mixes languages is analysed by several providers into one
//! model. Declaration IDs embed the project-relative file path, which keeps
//! providers from colliding.
//!
//! Adding a language means implementing [`LanguageProvider`] in its own crate
//! and registering it where the application is composed (the CLI); nothing in
//! the core, diagram, or TUI crates changes.

mod project_path;

use codeuml_core::Codebase;
use thiserror::Error;

pub use project_path::{ProjectPath, ProjectPathError};

/// What one provider contributed to the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProviderReport {
    pub files_analyzed: usize,
    pub files_failed: usize,
    /// True when the provider hit recoverable problems, such as unparsable
    /// files or discovery warnings, so the model may be incomplete.
    pub partial: bool,
}

/// A provider could not analyse the project at all. Recoverable per-file and
/// per-package problems are diagnostics in the [`Codebase`] instead.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct ProviderError {
    provider: &'static str,
    message: String,
}

impl ProviderError {
    pub fn new(provider: &'static str, cause: impl std::fmt::Display) -> Self {
        Self {
            provider,
            message: cause.to_string(),
        }
    }

    /// The [`LanguageProvider::id`] of the provider that failed.
    pub fn provider(&self) -> &'static str {
        self.provider
    }
}

/// Support for one source language.
pub trait LanguageProvider {
    /// Stable lowercase identifier, such as `"rust"`.
    fn id(&self) -> &'static str;

    /// Human-readable description of what makes a directory a project of this
    /// language, used in the "no supported project" message
    /// (for example `"Rust (Cargo.toml)"`).
    fn detection_hint(&self) -> &'static str;

    /// Whether `root` looks like a project of this language. This must be a
    /// cheap check (for instance looking for a manifest file); it must not
    /// parse sources or run external tools.
    fn detect(&self, root: &ProjectPath) -> bool;

    /// Analyse the project and add its files, modules, types, relationships,
    /// and diagnostics to `codebase`.
    fn analyze(
        &self,
        root: &ProjectPath,
        codebase: &mut Codebase,
    ) -> Result<ProviderReport, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_errors_keep_the_cause_text_and_provider_id() {
        let error = ProviderError::new("rust", "manifest is missing");
        assert_eq!(error.to_string(), "manifest is missing");
        assert_eq!(error.provider(), "rust");
    }

    #[test]
    fn reports_default_to_an_empty_complete_result() {
        assert_eq!(
            ProviderReport::default(),
            ProviderReport {
                files_analyzed: 0,
                files_failed: 0,
                partial: false
            }
        );
    }
}
