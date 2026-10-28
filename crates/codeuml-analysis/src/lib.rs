//! Language-neutral analysis orchestration.
//!
//! This crate picks the providers that recognise a project, runs them into one
//! shared model, and combines their reports. All language-specific work
//! (project discovery, parsing, reference resolution) lives in the provider
//! crates behind [`LanguageProvider`], so adding a language never changes this
//! crate.

mod analysis;

pub use analysis::{analyze_project, AnalysisError};
pub use codeuml_lang::{
    LanguageProvider, ProjectPath, ProjectPathError, ProviderError, ProviderReport,
};
