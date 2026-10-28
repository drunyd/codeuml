//! Language-neutral code structure model shared by analysis, diagrams, and UI.
//!
//! The crate intentionally has no dependency on the TUI, diagram renderer, or
//! a language parser. Analysis crates construct the model; consumers read it.
//!
//! Core invariants:
//! - declarations use stable [`SymbolId`]s rather than display names;
//! - insertion rejects duplicate declaration IDs;
//! - resolved relationship targets must reference declarations in the model;
//! - unresolved relationship targets are preserved explicitly;
//! - relationships are merged by `(source, target, kind)` while evidence and
//!   resolution/provenance remain separate observations.
//!
//! Source locations and file paths are project-relative.

mod analysis;
mod codebase;
mod declaration;
mod diagnostic;
mod error;
mod id;
mod language;
mod location;
mod relationship;

pub use analysis::{AnalysisMetadata, AnalysisMode};
pub use codebase::Codebase;
pub use declaration::{
    Field, GenericParam, Method, Module, Parameter, SourceFile, TypeDecl, TypeKind, Variant,
    Visibility,
};
pub use diagnostic::{Diagnostic, DiagnosticSeverity};
pub use error::ModelError;
pub use id::SymbolId;
pub use language::Language;
pub use location::SourceLocation;
pub use relationship::{
    Provenance, Relationship, RelationshipEvidence, RelationshipKind, RelationshipTarget,
    ResolutionStatus, UnresolvedReference,
};
