use std::fs;
use std::path::PathBuf;

use codeuml_core::Codebase;
use codeuml_lang::{LanguageProvider, ProjectPath};
use codeuml_lang_rust::RustProvider;

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codeuml-rust-provider-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    fn root(&self) -> ProjectPath {
        ProjectPath::resolve(Some(&self.0)).expect("project path")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn identifies_itself_and_what_it_looks_for() {
    assert_eq!(RustProvider.id(), "rust");
    assert_eq!(RustProvider.detection_hint(), "Rust (Cargo.toml)");
}

#[test]
fn detects_projects_by_their_cargo_manifest() {
    let dir = TempDir::new("detect");
    assert!(!RustProvider.detect(&dir.root()));

    fs::write(dir.0.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("manifest");
    assert!(RustProvider.detect(&dir.root()));
}

#[test]
fn analysis_fills_the_shared_model_and_reports_what_it_read() {
    let dir = TempDir::new("analyze");
    fs::create_dir_all(dir.0.join("src")).expect("src");
    fs::write(
        dir.0.join("Cargo.toml"),
        "[package]\nname = \"provider-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    fs::write(
        dir.0.join("src/lib.rs"),
        "pub struct Wrapper { inner: Inner }\npub struct Inner;\n",
    )
    .expect("source");

    let mut codebase = Codebase::new("fixture");
    let report = RustProvider
        .analyze(&dir.root(), &mut codebase)
        .expect("analysis");

    assert_eq!((report.files_analyzed, report.files_failed), (1, 0));
    assert!(!report.partial);
    assert_eq!(codebase.types().len(), 2);
    assert_eq!(codebase.relationships().len(), 1);
    assert_eq!(codebase.files().len(), 1);
}

#[test]
fn a_broken_source_file_makes_the_report_partial_but_not_fatal() {
    let dir = TempDir::new("partial");
    fs::create_dir_all(dir.0.join("src")).expect("src");
    fs::write(
        dir.0.join("Cargo.toml"),
        "[package]\nname = \"partial-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    fs::write(dir.0.join("src/lib.rs"), "mod broken;\npub struct Good;\n").expect("lib");
    fs::write(dir.0.join("src/broken.rs"), "pub struct {").expect("broken");

    let mut codebase = Codebase::new("fixture");
    let report = RustProvider
        .analyze(&dir.root(), &mut codebase)
        .expect("analysis");

    assert_eq!(report.files_failed, 1);
    assert!(report.partial);
    assert_eq!(codebase.types().len(), 1);
}

#[test]
fn a_project_without_a_manifest_is_a_provider_error_naming_cargo() {
    let dir = TempDir::new("missing");
    let mut codebase = Codebase::new("fixture");

    let error = RustProvider
        .analyze(&dir.root(), &mut codebase)
        .expect_err("no manifest");

    assert_eq!(error.provider(), "rust");
    assert!(error.to_string().contains("Cargo.toml"), "{error}");
}
