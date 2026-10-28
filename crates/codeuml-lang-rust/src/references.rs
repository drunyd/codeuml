use codeuml_core::{RelationshipKind, SourceLocation, SymbolId};

/// Root interpretation for a Rust type/use path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RustPathRoot {
    /// A path written relative to the current module, e.g. `Customer`.
    Relative,
    /// A path beginning with `crate::`.
    Crate,
    /// A path beginning with `self::`.
    SelfModule,
    /// A path beginning with one or more `super::` segments.
    Super(usize),
    /// A path beginning with `::`.
    Global,
    /// A qualified expression that cannot be represented as a simple path,
    /// such as `<T as Trait>::Item`.
    Unresolved,
}

/// A Rust path extracted from a type expression or import.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RustTypePath {
    pub root: RustPathRoot,
    pub segments: Vec<String>,
    /// Source-like display text retained for evidence and unresolved targets.
    pub display: String,
}

/// A `use` binding visible in one Rust module.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RustImport {
    Named { alias: String, path: RustTypePath },
    Glob { path: RustTypePath },
}

/// A language-specific, unresolved reference emitted by the Rust analyzer.
///
/// The analysis crate turns these into core relationships after building a
/// project-wide declaration index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustReference {
    /// Owning type or member declaration. Member sources are mapped back to
    /// their owner by diagram projection.
    pub source: SymbolId,
    pub kind: RelationshipKind,
    pub path: RustTypePath,
    pub location: SourceLocation,
    /// Crate identity used for module-context resolution.
    pub crate_key: String,
    /// Module path segments below the crate root.
    pub module_path: Vec<String>,
    /// Type/const generic parameter names that must not resolve to project types.
    pub generic_parameters: Vec<String>,
    /// Imports visible in the reference's module.
    pub imports: Vec<RustImport>,
    pub note: Option<String>,
}
