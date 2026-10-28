use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use codeuml_core::{Diagnostic, DiagnosticSeverity};
use serde_json::Value;
use thiserror::Error;

use crate::ProjectPath;

const EXCLUDED_DIRECTORIES: &[&str] = &["target", ".git", "vendor"];

/// A Cargo package included in the discovered workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredPackage {
    pub name: String,
    /// Project-relative path to the package manifest.
    pub manifest_path: PathBuf,
    /// Project-relative package root. `"."` for a package at the project root.
    pub root: PathBuf,
    pub targets: Vec<DiscoveredTarget>,
    /// Project-relative, normalized, sorted source paths owned by this package.
    pub source_files: Vec<PathBuf>,
}

/// A Cargo target and its declared source root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredTarget {
    pub name: String,
    /// Original Cargo target kinds such as `lib`, `bin`, or `test`.
    pub kinds: Vec<String>,
    /// Project-relative path to the target source file.
    pub source_path: PathBuf,
}

/// Rust sources and package metadata discovered from a Cargo project.
///
/// Discovery does not parse source files. Malformed sources are still listed so
/// the analyzer can report per-file diagnostics while keeping other results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredProject {
    root: PathBuf,
    packages: Vec<DiscoveredPackage>,
    source_files: Vec<PathBuf>,
    diagnostics: Vec<Diagnostic>,
}

struct DiscoveryResults {
    packages: Vec<DiscoveredPackage>,
    source_files: BTreeSet<PathBuf>,
    diagnostics: Vec<Diagnostic>,
}

struct TargetDiscovery {
    targets: Vec<DiscoveredTarget>,
    support_roots: BTreeSet<PathBuf>,
    source_files: BTreeSet<PathBuf>,
}

impl DiscoveredProject {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn packages(&self) -> &[DiscoveredPackage] {
        &self.packages
    }

    /// Project-relative, normalized, sorted source paths.
    pub fn source_files(&self) -> &[PathBuf] {
        &self.source_files
    }

    /// Recoverable discovery problems. Fatal Cargo/manifest problems are errors.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Discover Cargo packages and Rust source files under an already resolved
/// project root.
///
/// Uses `cargo metadata --no-deps`, so dependency downloads and compilation are
/// not required. Projects with invalid manifests or an unavailable Cargo
/// executable return a clear error.
pub fn discover_project(project: &ProjectPath) -> Result<DiscoveredProject, DiscoveryError> {
    discover_project_with_cargo(project, Path::new("cargo"))
}

fn discover_project_with_cargo(
    project: &ProjectPath,
    cargo: &Path,
) -> Result<DiscoveredProject, DiscoveryError> {
    let root = project.as_path();
    let manifest = root.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(DiscoveryError::ManifestMissing(manifest));
    }
    let metadata = load_metadata(project, cargo, &manifest)?;
    let workspace_members = parse_workspace_members(&metadata)?;
    let package_metadata = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or(DiscoveryError::MetadataShape { field: "packages" })?;
    let discovered = discover_packages(root, package_metadata, &workspace_members)?;
    if discovered.packages.is_empty() {
        return Err(DiscoveryError::NoPackages(root.to_path_buf()));
    }
    Ok(DiscoveredProject {
        root: root.to_path_buf(),
        packages: discovered.packages,
        source_files: discovered.source_files.into_iter().collect(),
        diagnostics: discovered.diagnostics,
    })
}

fn parse_workspace_members(metadata: &Value) -> Result<BTreeSet<String>, DiscoveryError> {
    let members = metadata
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or(DiscoveryError::MetadataShape {
            field: "workspace_members",
        })?;
    let mut parsed = BTreeSet::new();
    for member in members {
        let member = member.as_str().ok_or(DiscoveryError::MetadataShape {
            field: "workspace_members[]",
        })?;
        parsed.insert(member.to_owned());
    }
    Ok(parsed)
}

fn discover_packages(
    root: &Path,
    package_metadata: &[Value],
    workspace_members: &BTreeSet<String>,
) -> Result<DiscoveryResults, DiscoveryError> {
    let mut packages = Vec::new();
    let mut source_files = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for package in package_metadata {
        if let Some(discovered) =
            discover_package(root, package, workspace_members, &mut diagnostics)?
        {
            source_files.extend(discovered.source_files.iter().cloned());
            packages.push(discovered);
        }
    }
    packages.sort_by(|left, right| {
        left.manifest_path
            .cmp(&right.manifest_path)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(DiscoveryResults {
        packages,
        source_files,
        diagnostics,
    })
}

fn discover_package(
    root: &Path,
    package: &Value,
    workspace_members: &BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<DiscoveredPackage>, DiscoveryError> {
    let package_id = required_str(package, "id", "packages[].id")?;
    if !workspace_members.contains(package_id) {
        return Ok(None);
    }
    let name = required_str(package, "name", "packages[].name")?;
    let manifest = PathBuf::from(required_str(
        package,
        "manifest_path",
        "packages[].manifest_path",
    )?);
    let Some((canonical_root, package_root, manifest_path)) =
        discover_package_paths(root, name, &manifest, diagnostics)
    else {
        return Ok(None);
    };
    let targets =
        package
            .get("targets")
            .and_then(Value::as_array)
            .ok_or(DiscoveryError::MetadataShape {
                field: "packages[].targets",
            })?;
    let target_discovery = discover_targets(root, targets, diagnostics)?;
    let discovered_targets = target_discovery.targets;
    let support_roots = target_discovery.support_roots;
    let mut package_sources = target_discovery.source_files;
    collect_package_sources(
        root,
        &canonical_root,
        &support_roots,
        &mut package_sources,
        diagnostics,
    );
    Ok(Some(DiscoveredPackage {
        name: name.to_owned(),
        manifest_path,
        root: package_root,
        targets: discovered_targets,
        source_files: package_sources.into_iter().collect(),
    }))
}

fn discover_package_paths(
    root: &Path,
    name: &str,
    manifest: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let Some(package_root) = manifest.parent() else {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping package `{name}` because its manifest has no parent directory: {}",
            manifest.display()
        )));
        return None;
    };
    let canonical_root = match fs::canonicalize(package_root) {
        Ok(path) => path,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "skipping package `{name}` at `{}`: {error}",
                package_root.display()
            )));
            return None;
        }
    };
    if !canonical_root.starts_with(root) {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping package `{name}` outside the project root: {}",
            package_root.display()
        )));
        return None;
    }
    let Some(relative_root) = relative_path(root, &canonical_root) else {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping package `{name}` because its path cannot be made project-relative"
        )));
        return None;
    };
    let Some(relative_manifest) = relative_path(root, manifest) else {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping package `{name}` because its manifest path cannot be made project-relative"
        )));
        return None;
    };
    Some((canonical_root, relative_root, relative_manifest))
}

fn discover_targets(
    root: &Path,
    targets: &[Value],
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<TargetDiscovery, DiscoveryError> {
    let mut discovered = Vec::new();
    let mut support_roots = BTreeSet::new();
    let mut source_files = BTreeSet::new();
    for target in targets {
        if let Some((discovered_target, source_abs)) = discover_target(root, target, diagnostics)? {
            if let Some(parent) = source_abs.parent() {
                if parent.starts_with(root) {
                    support_roots.insert(parent.to_path_buf());
                }
            }
            source_files.insert(discovered_target.source_path.clone());
            discovered.push(discovered_target);
        }
    }
    discovered.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(TargetDiscovery {
        targets: discovered,
        support_roots,
        source_files,
    })
}

fn discover_target(
    root: &Path,
    target: &Value,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<(DiscoveredTarget, PathBuf)>, DiscoveryError> {
    let name = required_str(target, "name", "packages[].targets[].name")?;
    let kinds =
        target
            .get("kind")
            .and_then(Value::as_array)
            .ok_or(DiscoveryError::MetadataShape {
                field: "packages[].targets[].kind",
            })?;
    let kinds = parse_target_kinds(kinds)?;
    let source_abs = PathBuf::from(required_str(
        target,
        "src_path",
        "packages[].targets[].src_path",
    )?);
    let Some(source_path) = checked_source_path(root, &source_abs, diagnostics) else {
        return Ok(None);
    };
    Ok(Some((
        DiscoveredTarget {
            name: name.to_owned(),
            kinds,
            source_path,
        },
        source_abs,
    )))
}

fn parse_target_kinds(kinds: &[Value]) -> Result<Vec<String>, DiscoveryError> {
    let mut parsed = Vec::new();
    for kind in kinds {
        let kind = kind.as_str().ok_or(DiscoveryError::MetadataShape {
            field: "packages[].targets[].kind[]",
        })?;
        parsed.push(kind.to_owned());
    }
    Ok(parsed)
}

fn collect_package_sources(
    root: &Path,
    package_root: &Path,
    support_roots: &BTreeSet<PathBuf>,
    sources: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source_directory = package_root.join("src");
    if source_directory.is_dir() {
        walk_source_directory(root, &source_directory, sources, diagnostics);
    }
    for support_root in support_roots {
        collect_target_support(root, package_root, support_root, sources, diagnostics);
    }
}

fn load_metadata(
    project: &ProjectPath,
    cargo: &Path,
    manifest: &Path,
) -> Result<Value, DiscoveryError> {
    let output = Command::new(cargo)
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(project.as_path())
        .output()
        .map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                DiscoveryError::CargoUnavailable {
                    cargo: cargo.to_path_buf(),
                    source,
                }
            } else {
                DiscoveryError::CargoExecution {
                    cargo: cargo.to_path_buf(),
                    source,
                }
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = stderr.trim();
        return Err(DiscoveryError::CargoMetadataFailed {
            manifest: manifest.to_path_buf(),
            message: if message.is_empty() {
                format!("Cargo exited with {}", output.status)
            } else {
                message.to_owned()
            },
        });
    }

    serde_json::from_slice(&output.stdout).map_err(DiscoveryError::InvalidCargoMetadata)
}

fn required_str<'a>(
    value: &'a Value,
    field: &str,
    metadata_field: &'static str,
) -> Result<&'a str, DiscoveryError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(DiscoveryError::MetadataShape {
            field: metadata_field,
        })
}

fn checked_source_path(
    root: &Path,
    source: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PathBuf> {
    if !inspect_source_link(source, diagnostics) {
        return None;
    }
    normalize_checked_source(root, source, diagnostics)
}

fn inspect_source_link(source: &Path, diagnostics: &mut Vec<Diagnostic>) -> bool {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not inspect source file `{}`: {error}",
                source.display()
            )));
            return false;
        }
    };
    if metadata.file_type().is_symlink() {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping symbolic link `{}`",
            source.display()
        )));
        return false;
    }
    true
}

fn normalize_checked_source(
    root: &Path,
    source: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PathBuf> {
    let canonical = match fs::canonicalize(source) {
        Ok(path) => path,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not resolve source file `{}`: {error}",
                source.display()
            )));
            return None;
        }
    };
    if !canonical.starts_with(root) {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping source file outside the project root: {}",
            source.display()
        )));
        return None;
    }
    relative_source_path(root, &canonical, diagnostics)
}

fn relative_source_path(
    root: &Path,
    source: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PathBuf> {
    let Some(relative) = relative_path(root, source) else {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping source file with a non-normalizable path: {}",
            source.display()
        )));
        return None;
    };
    Some(relative)
}

/// Collect a crate root's direct Rust files plus conventional module
/// directories. This avoids treating arbitrary fixture directories as source.
fn collect_target_support(
    root: &Path,
    package_root: &Path,
    target_root: &Path,
    source_files: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !target_root.starts_with(package_root)
        || !inspect_directory(target_root, "target", diagnostics)
    {
        return;
    }
    let Some(entries) = read_directory(target_root, "target", diagnostics) else {
        return;
    };
    for entry in entries {
        process_target_entry(root, target_root, entry, source_files, diagnostics);
    }
}

fn inspect_directory(directory: &Path, label: &str, diagnostics: &mut Vec<Diagnostic>) -> bool {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not inspect {label} directory `{}`: {error}",
                directory.display()
            )));
            return false;
        }
    };
    if metadata.file_type().is_symlink() {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping symbolic link `{}`",
            directory.display()
        )));
        return false;
    }
    metadata.is_dir()
}

fn read_directory(
    directory: &Path,
    label: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<fs::ReadDir> {
    match fs::read_dir(directory) {
        Ok(entries) => Some(entries),
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not read {label} directory `{}`: {error}",
                directory.display()
            )));
            None
        }
    }
}

fn process_target_entry(
    root: &Path,
    target_root: &Path,
    entry: std::io::Result<fs::DirEntry>,
    source_files: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let entry = match entry {
        Ok(entry) => entry,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not read an entry in `{}`: {error}",
                target_root.display()
            )));
            return;
        }
    };
    let path = entry.path();
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not inspect `{}`: {error}",
                path.display()
            )));
            return;
        }
    };
    if file_type.is_symlink() {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping symbolic link `{}`",
            path.display()
        )));
        return;
    }
    if file_type.is_dir() {
        collect_conventional_directory(root, &path, source_files, diagnostics);
        return;
    }
    if is_rust_file(&path, &file_type) {
        add_target_source(root, target_root, &path, source_files, diagnostics);
    }
}

fn collect_conventional_directory(
    root: &Path,
    directory: &Path,
    source_files: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if is_excluded_directory(directory) {
        return;
    }
    if directory.join("mod.rs").is_file() {
        walk_source_directory(root, directory, source_files, diagnostics);
    }
}

fn is_rust_file(path: &Path, file_type: &fs::FileType) -> bool {
    file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs")
}

fn add_target_source(
    root: &Path,
    target_root: &Path,
    path: &Path,
    source_files: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(relative) = relative_source_path(root, path, diagnostics) {
        source_files.insert(relative);
    }
    if let Some(stem) = path.file_stem() {
        let child_directory = target_root.join(stem);
        if child_directory.is_dir() && !is_excluded_directory(&child_directory) {
            walk_source_directory(root, &child_directory, source_files, diagnostics);
        }
    }
}

fn walk_source_directory(
    root: &Path,
    directory: &Path,
    source_files: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !inspect_directory(directory, "source", diagnostics) {
        return;
    }
    let Some(entries) = read_directory(directory, "source", diagnostics) else {
        return;
    };
    for entry in entries {
        process_source_entry(root, directory, entry, source_files, diagnostics);
    }
}

fn process_source_entry(
    root: &Path,
    directory: &Path,
    entry: std::io::Result<fs::DirEntry>,
    source_files: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some((path, file_type)) = source_entry_info(directory, entry, diagnostics) else {
        return;
    };
    if file_type.is_dir() {
        if !is_excluded_directory(&path) {
            walk_source_directory(root, &path, source_files, diagnostics);
        }
        return;
    }
    if is_rust_file(&path, &file_type) {
        if let Some(relative) = relative_source_path(root, &path, diagnostics) {
            source_files.insert(relative);
        }
    }
}

fn source_entry_info(
    directory: &Path,
    entry: std::io::Result<fs::DirEntry>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(PathBuf, fs::FileType)> {
    let entry = match entry {
        Ok(entry) => entry,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not read an entry in `{}`: {error}",
                directory.display()
            )));
            return None;
        }
    };
    let path = entry.path();
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not inspect `{}`: {error}",
                path.display()
            )));
            return None;
        }
    };
    if file_type.is_symlink() {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping symbolic link `{}`",
            path.display()
        )));
        return None;
    }
    Some((path, file_type))
}

fn is_excluded_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| EXCLUDED_DIRECTORIES.contains(&name))
}

fn relative_path(root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(root).ok()?;
    let mut normalized = String::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push_str(&part.to_string_lossy());
            }
            Component::CurDir => {}
            _ => return None,
        }
    }

    if normalized.is_empty() {
        Some(PathBuf::from("."))
    } else {
        Some(PathBuf::from(normalized))
    }
}

fn discovery_diagnostic(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(DiagnosticSeverity::Warning, message, None)
}

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("no Cargo manifest found at `{}`", .0.display())]
    ManifestMissing(PathBuf),
    #[error("could not run Cargo (`{}`): {source}", cargo.display())]
    CargoUnavailable {
        cargo: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not execute Cargo (`{}`): {source}", cargo.display())]
    CargoExecution {
        cargo: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Cargo metadata failed for `{}`: {message}", manifest.display())]
    CargoMetadataFailed { manifest: PathBuf, message: String },
    #[error("Cargo metadata returned invalid JSON: {0}")]
    InvalidCargoMetadata(#[source] serde_json::Error),
    #[error("Cargo metadata is missing required field `{field}`")]
    MetadataShape { field: &'static str },
    #[error("the Cargo project at `{}` contains no workspace packages", .0.display())]
    NoPackages(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn missing_cargo_executable_is_reported() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        static LOCAL_ID: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "codeuml-missing-cargo-{}-{}-{}",
            std::process::id(),
            id,
            LOCAL_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temporary project");
        fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"missing-cargo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("write manifest");
        fs::create_dir_all(path.join("src")).expect("create source directory");
        fs::write(path.join("src/lib.rs"), "pub struct Example;\n").expect("write source");

        let project = ProjectPath::resolve(Some(&path)).expect("resolve project");
        let error = discover_project_with_cargo(&project, &path.join("this-cargo-does-not-exist"))
            .expect_err("missing cargo must fail");

        assert!(matches!(error, DiscoveryError::CargoUnavailable { .. }));
        assert!(error.to_string().contains("could not run Cargo"));

        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn invalid_source_and_support_paths_produce_diagnostics() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codeuml-discovery-paths-{}-{id}",
            std::process::id()
        ));
        let outside = root.with_extension("outside.rs");
        fs::create_dir_all(&root).expect("create project root");
        fs::write(&outside, "pub struct Outside;\n").expect("write outside source");
        let not_directory = root.join("not-directory");
        fs::write(&not_directory, "file").expect("write non-directory");
        let root = fs::canonicalize(&root).expect("canonical project root");
        let mut diagnostics = Vec::new();

        assert!(checked_source_path(&root, &root.join("missing.rs"), &mut diagnostics).is_none());
        assert!(checked_source_path(&root, &outside, &mut diagnostics).is_none());
        collect_target_support(
            &root,
            &root,
            &root.join("missing-target"),
            &mut BTreeSet::new(),
            &mut diagnostics,
        );
        walk_source_directory(
            &root,
            &root.join("missing-source"),
            &mut BTreeSet::new(),
            &mut diagnostics,
        );
        assert!(read_directory(&not_directory, "source", &mut diagnostics).is_none());
        process_target_entry(
            &root,
            &root,
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "simulated unreadable entry",
            )),
            &mut BTreeSet::new(),
            &mut diagnostics,
        );
        process_source_entry(
            &root,
            &root,
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "simulated unreadable entry",
            )),
            &mut BTreeSet::new(),
            &mut diagnostics,
        );

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("outside the project root")));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("could not inspect target directory")));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("could not inspect source directory")));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("could not read source directory")));
        assert!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.message.contains("could not read an entry in"))
                .count()
                >= 2
        );
        let _ = fs::remove_file(outside);
        let _ = fs::remove_dir_all(root);
    }
}
