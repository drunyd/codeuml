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
    let workspace_members = metadata
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or(DiscoveryError::MetadataShape {
            field: "workspace_members",
        })?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or(DiscoveryError::MetadataShape {
                    field: "workspace_members[]",
                })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;

    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or(DiscoveryError::MetadataShape { field: "packages" })?;

    let mut diagnostics = Vec::new();
    let mut discovered_packages = Vec::new();
    let mut source_files = BTreeSet::new();

    for package in packages {
        let package_id = required_str(package, "id", "packages[].id")?;
        if !workspace_members.contains(package_id) {
            continue;
        }

        let name = required_str(package, "name", "packages[].name")?;
        let manifest_abs = PathBuf::from(required_str(
            package,
            "manifest_path",
            "packages[].manifest_path",
        )?);
        let package_root_abs = match manifest_abs.parent() {
            Some(parent) => parent.to_path_buf(),
            None => {
                diagnostics.push(discovery_diagnostic(format!(
                    "skipping package `{name}` because its manifest has no parent directory: {}",
                    manifest_abs.display()
                )));
                continue;
            }
        };

        let canonical_package_root = match fs::canonicalize(&package_root_abs) {
            Ok(path) => path,
            Err(error) => {
                diagnostics.push(discovery_diagnostic(format!(
                    "skipping package `{name}` at `{}`: {error}",
                    package_root_abs.display()
                )));
                continue;
            }
        };
        if !canonical_package_root.starts_with(root) {
            diagnostics.push(discovery_diagnostic(format!(
                "skipping package `{name}` outside the project root: {}",
                package_root_abs.display()
            )));
            continue;
        }

        let Some(package_root_relative) = relative_path(root, &canonical_package_root) else {
            diagnostics.push(discovery_diagnostic(format!(
                "skipping package `{name}` because its path cannot be made project-relative"
            )));
            continue;
        };
        let Some(manifest_relative) = relative_path(root, &manifest_abs) else {
            diagnostics.push(discovery_diagnostic(format!(
                "skipping package `{name}` because its manifest path cannot be made project-relative"
            )));
            continue;
        };

        let targets = package.get("targets").and_then(Value::as_array).ok_or(
            DiscoveryError::MetadataShape {
                field: "packages[].targets",
            },
        )?;

        let mut discovered_targets = Vec::new();
        let mut target_support_roots = BTreeSet::new();
        let mut package_source_files = BTreeSet::new();
        for target in targets {
            let target_name = required_str(target, "name", "packages[].targets[].name")?;
            let kinds = target
                .get("kind")
                .and_then(Value::as_array)
                .ok_or(DiscoveryError::MetadataShape {
                    field: "packages[].targets[].kind",
                })?
                .iter()
                .map(|kind| {
                    kind.as_str()
                        .map(ToOwned::to_owned)
                        .ok_or(DiscoveryError::MetadataShape {
                            field: "packages[].targets[].kind[]",
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let source_abs = PathBuf::from(required_str(
                target,
                "src_path",
                "packages[].targets[].src_path",
            )?);

            let Some(relative_source) = checked_source_path(root, &source_abs, &mut diagnostics)
            else {
                continue;
            };
            discovered_targets.push(DiscoveredTarget {
                name: target_name.to_owned(),
                kinds,
                source_path: relative_source.clone(),
            });
            package_source_files.insert(relative_source.clone());
            source_files.insert(relative_source);

            if let Some(parent) = source_abs.parent() {
                if parent.starts_with(root) {
                    target_support_roots.insert(parent.to_path_buf());
                }
            }
        }
        discovered_targets.sort_by(|left, right| {
            left.source_path
                .cmp(&right.source_path)
                .then_with(|| left.name.cmp(&right.name))
        });

        // `src` is the package's module tree, so all Rust files below it are
        // source candidates. Other Cargo target roots are handled below without
        // recursively sweeping unrelated fixture/asset directories.
        let src_directory = canonical_package_root.join("src");
        if src_directory.is_dir() {
            walk_source_directory(
                root,
                &src_directory,
                &mut package_source_files,
                &mut diagnostics,
            );
        }

        for support_root in &target_support_roots {
            collect_target_support(
                root,
                &canonical_package_root,
                support_root,
                &mut package_source_files,
                &mut diagnostics,
            );
        }
        source_files.extend(package_source_files.iter().cloned());

        discovered_packages.push(DiscoveredPackage {
            name: name.to_owned(),
            manifest_path: manifest_relative,
            root: package_root_relative,
            targets: discovered_targets,
            source_files: package_source_files.into_iter().collect(),
        });
    }

    discovered_packages.sort_by(|left, right| {
        left.manifest_path
            .cmp(&right.manifest_path)
            .then_with(|| left.name.cmp(&right.name))
    });

    if discovered_packages.is_empty() {
        return Err(DiscoveryError::NoPackages(root.to_path_buf()));
    }

    Ok(DiscoveredProject {
        root: root.to_path_buf(),
        packages: discovered_packages,
        source_files: source_files.into_iter().collect(),
        diagnostics,
    })
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
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not inspect source file `{}`: {error}",
                source.display()
            )));
            return None;
        }
    };
    if metadata.file_type().is_symlink() {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping symbolic link `{}`",
            source.display()
        )));
        return None;
    }

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

    let Some(relative) = relative_path(root, &canonical) else {
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
    if !target_root.starts_with(package_root) {
        return;
    }
    let target_metadata = match fs::symlink_metadata(target_root) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not inspect target directory `{}`: {error}",
                target_root.display()
            )));
            return;
        }
    };
    if target_metadata.file_type().is_symlink() {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping symbolic link `{}`",
            target_root.display()
        )));
        return;
    }
    if !target_metadata.is_dir() {
        return;
    }

    let entries = match fs::read_dir(target_root) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not read target directory `{}`: {error}",
                target_root.display()
            )));
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(discovery_diagnostic(format!(
                    "could not read an entry in `{}`: {error}",
                    target_root.display()
                )));
                continue;
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
                continue;
            }
        };
        if file_type.is_symlink() {
            diagnostics.push(discovery_diagnostic(format!(
                "skipping symbolic link `{}`",
                path.display()
            )));
            continue;
        }

        if file_type.is_dir() {
            if is_excluded_directory(&path) {
                continue;
            }
            // A `mod.rs` marks a conventional module directory.
            if path.join("mod.rs").is_file() {
                walk_source_directory(root, &path, source_files, diagnostics);
            }
            continue;
        }

        if !file_type.is_file() || path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        if let Some(relative) = relative_path(root, &path) {
            source_files.insert(relative);
        } else {
            diagnostics.push(discovery_diagnostic(format!(
                "skipping source file with a non-normalizable path: {}",
                path.display()
            )));
        }

        // Child modules of `foo.rs` live in a sibling `foo/` directory.
        if let Some(stem) = path.file_stem() {
            let child_directory = target_root.join(stem);
            if child_directory.is_dir() && !is_excluded_directory(&child_directory) {
                walk_source_directory(root, &child_directory, source_files, diagnostics);
            }
        }
    }
}

fn walk_source_directory(
    root: &Path,
    directory: &Path,
    source_files: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let directory_metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not inspect source directory `{}`: {error}",
                directory.display()
            )));
            return;
        }
    };
    if directory_metadata.file_type().is_symlink() {
        diagnostics.push(discovery_diagnostic(format!(
            "skipping symbolic link `{}`",
            directory.display()
        )));
        return;
    }
    if !directory_metadata.is_dir() {
        return;
    }

    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(discovery_diagnostic(format!(
                "could not read source directory `{}`: {error}",
                directory.display()
            )));
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(discovery_diagnostic(format!(
                    "could not read an entry in `{}`: {error}",
                    directory.display()
                )));
                continue;
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
                continue;
            }
        };

        if file_type.is_symlink() {
            diagnostics.push(discovery_diagnostic(format!(
                "skipping symbolic link `{}`",
                path.display()
            )));
            continue;
        }

        if file_type.is_dir() {
            if is_excluded_directory(&path) {
                continue;
            }
            walk_source_directory(root, &path, source_files, diagnostics);
            continue;
        }

        if file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
            let Some(relative) = relative_path(root, &path) else {
                diagnostics.push(discovery_diagnostic(format!(
                    "skipping source file with a non-normalizable path: {}",
                    path.display()
                )));
                continue;
            };
            source_files.insert(relative);
        }
    }
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
}
