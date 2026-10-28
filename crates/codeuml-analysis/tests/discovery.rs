use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use codeuml_analysis::{discover_project, DiscoveryError, ProjectPath};

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct TempProject {
    path: PathBuf,
}

impl TempProject {
    fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codeuml-discovery-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary project");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    fn project_path(&self) -> ProjectPath {
        ProjectPath::resolve(Some(&self.path)).expect("resolve fixture project")
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn relative_paths(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn write_package(project: &TempProject, directory: &str, name: &str, source: &str) {
    let manifest = if directory.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{directory}/Cargo.toml")
    };
    let source_path = if directory.is_empty() {
        "src/lib.rs".to_owned()
    } else {
        format!("{directory}/src/lib.rs")
    };
    project.write(
        &manifest,
        &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    );
    project.write(&source_path, source);
}

#[test]
fn discovers_a_single_package_and_its_conventional_sources() {
    let project = TempProject::new("single");
    write_package(&project, "", "single-app", "mod extra;\npub struct Root;\n");
    project.write("src/extra.rs", "pub fn helper() {}\n");
    project.write("scripts/not-a-cargo-source.rs", "fn unrelated() {}\n");

    let discovered = discover_project(&project.project_path()).expect("discover project");

    assert_eq!(discovered.packages().len(), 1);
    assert_eq!(discovered.packages()[0].name, "single-app");
    assert_eq!(
        relative_paths(discovered.source_files()),
        vec!["src/extra.rs", "src/lib.rs"]
    );
    assert!(discovered.diagnostics().is_empty());
}

#[test]
fn discovers_all_workspace_members() {
    let project = TempProject::new("workspace");
    project.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"alpha\", \"beta dir\"]\nresolver = \"2\"\n",
    );
    write_package(&project, "alpha", "alpha", "pub struct Alpha;\n");
    write_package(
        &project,
        "beta dir",
        "beta",
        "pub struct Beta;\nmod nested;\n",
    );
    project.write("beta dir/src/nested.rs", "pub struct Nested;\n");

    let discovered = discover_project(&project.project_path()).expect("discover workspace");

    let package_names: Vec<_> = discovered
        .packages()
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    assert_eq!(package_names, vec!["alpha", "beta"]);
    assert_eq!(
        relative_paths(&discovered.packages()[0].source_files),
        vec!["alpha/src/lib.rs"]
    );
    assert_eq!(
        relative_paths(&discovered.packages()[1].source_files),
        vec!["beta dir/src/lib.rs", "beta dir/src/nested.rs"]
    );
    assert_eq!(
        relative_paths(discovered.source_files()),
        vec![
            "alpha/src/lib.rs",
            "beta dir/src/lib.rs",
            "beta dir/src/nested.rs"
        ]
    );
}

#[test]
fn excludes_target_git_and_vendor_directories_inside_a_package() {
    let project = TempProject::new("excluded");
    write_package(&project, "", "excluded", "pub struct Included;\n");
    project.write("target/generated.rs", "pub struct Generated;\n");
    project.write(".git/hidden.rs", "pub struct Hidden;\n");
    project.write("vendor/dep/src/lib.rs", "pub struct Vendored;\n");
    project.write("src/vendor/inner.rs", "pub struct InnerVendor;\n");

    let discovered = discover_project(&project.project_path()).expect("discover project");

    assert_eq!(
        relative_paths(discovered.source_files()),
        vec!["src/lib.rs"]
    );
}

#[test]
fn includes_cargo_targets_outside_the_conventional_src_directory() {
    let project = TempProject::new("targets");
    project.write(
        "Cargo.toml",
        r#"[package]
name = "targets"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "custom"
path = "tools/custom.rs"
"#,
    );
    project.write("src/lib.rs", "pub struct Library;\n");
    project.write("src/main.rs", "fn main() {}\n");
    project.write("src/bin/tool.rs", "fn main() {}\n");
    project.write("tests/integration.rs", "#[test]\nfn it_works() {}\n");
    project.write("tests/common/mod.rs", "pub fn helper() {}\n");
    project.write("examples/demo.rs", "fn main() {}\n");
    project.write("benches/perf.rs", "fn main() {}\n");
    project.write("build.rs", "fn main() {}\n");
    project.write("tools/custom.rs", "fn main() {}\n");
    project.write("scripts/unrelated.rs", "fn unrelated() {}\n");
    project.write(
        "tests/fixtures/nested-project/src/lib.rs",
        "pub struct FixtureOnly;\n",
    );
    project.write("examples/fixtures/nested.rs", "fn nested() {}\n");

    let discovered = discover_project(&project.project_path()).expect("discover project");

    assert_eq!(
        relative_paths(discovered.source_files()),
        vec![
            "benches/perf.rs",
            "build.rs",
            "examples/demo.rs",
            "src/bin/tool.rs",
            "src/lib.rs",
            "src/main.rs",
            "tests/common/mod.rs",
            "tests/integration.rs",
            "tools/custom.rs",
        ]
    );
}

#[test]
fn discovers_a_package_that_lives_in_a_vendor_directory() {
    let project = TempProject::new("vendor-member");
    project.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"vendor/legit\"]\nresolver = \"2\"\n",
    );
    write_package(
        &project,
        "vendor/legit",
        "legit",
        "pub struct Legit;\nmod nested;\n",
    );
    project.write("vendor/legit/src/nested.rs", "pub struct Nested;\n");

    let discovered = discover_project(&project.project_path()).expect("discover workspace");

    assert_eq!(
        relative_paths(discovered.source_files()),
        vec!["vendor/legit/src/lib.rs", "vendor/legit/src/nested.rs"]
    );
}

#[test]
fn missing_manifest_is_reported() {
    let project = TempProject::new("missing-manifest");

    let error = discover_project(&project.project_path()).expect_err("manifest is required");

    assert!(matches!(error, DiscoveryError::ManifestMissing(_)));
    assert!(error.to_string().contains("Cargo.toml"));
}

#[test]
fn a_workspace_without_packages_is_rejected() {
    let project = TempProject::new("empty-workspace");
    project.write(
        "Cargo.toml",
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    );

    let error = discover_project(&project.project_path()).expect_err("no packages");

    assert!(matches!(error, DiscoveryError::NoPackages(_)));
}

#[test]
fn invalid_manifest_is_reported_as_a_cargo_failure() {
    let project = TempProject::new("invalid-manifest");
    project.write("Cargo.toml", "[package\nname = broken\n");

    let error = discover_project(&project.project_path()).expect_err("invalid manifest fails");

    assert!(matches!(error, DiscoveryError::CargoMetadataFailed { .. }));
    let message = error.to_string();
    assert!(message.contains("Cargo metadata failed"), "{message}");
}

#[test]
fn paths_with_spaces_are_supported() {
    let project = TempProject::new("path with spaces");
    write_package(
        &project,
        "",
        "spaced",
        "mod nested module;\npub struct Root;\n",
    );
    project.write("src/nested module.rs", "pub struct Nested;\n");

    let discovered = discover_project(&project.project_path()).expect("discover spaced project");

    assert_eq!(
        relative_paths(discovered.source_files()),
        vec!["src/lib.rs", "src/nested module.rs"]
    );
}

#[test]
fn malformed_source_files_are_still_discovered() {
    let project = TempProject::new("malformed");
    write_package(
        &project,
        "",
        "malformed",
        "mod broken;\npub struct Valid;\n",
    );
    project.write("src/broken.rs", "pub struct Broken { this is not Rust\n");

    let discovered = discover_project(&project.project_path()).expect("discovery does not parse");

    assert_eq!(
        relative_paths(discovered.source_files()),
        vec!["src/broken.rs", "src/lib.rs"]
    );
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_not_followed() {
    use std::os::unix::fs::symlink;

    let project = TempProject::new("symlink");
    write_package(&project, "", "symlinked", "pub struct Root;\n");
    let outside = TempProject::new("symlink-outside");
    outside.write("outside.rs", "pub struct Outside;\n");
    symlink(
        outside.path().join("outside.rs"),
        project.path().join("src/outside.rs"),
    )
    .expect("create symlink");

    let discovered = discover_project(&project.project_path()).expect("discover project");

    assert_eq!(
        relative_paths(discovered.source_files()),
        vec!["src/lib.rs"]
    );
    assert!(discovered
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.message.contains("symbolic link")));
}
