use std::path::PathBuf;

/// Rust source inputs for a single Cargo package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustPackageSources {
    pub name: String,
    /// Project-relative source files belonging to this package.
    pub source_files: Vec<PathBuf>,
    /// Cargo targets define crate roots.
    pub targets: Vec<RustTargetSource>,
}

/// A Cargo target that acts as a crate root for module traversal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustTargetSource {
    pub name: String,
    /// Project-relative path to the crate root source file.
    pub source_path: PathBuf,
    pub is_library: bool,
}
