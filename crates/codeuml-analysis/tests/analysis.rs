use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use codeuml_analysis::{analyze_project, AnalysisError, ProjectPath};
use codeuml_core::{AnalysisMode, Codebase, ResolutionStatus};

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct TempProject {
    path: PathBuf,
}

impl TempProject {
    fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codeuml-analysis-{label}-{}-{id}",
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

    fn analyze(&self) -> Codebase {
        let project = ProjectPath::resolve(Some(&self.path)).expect("resolve project");
        analyze_project(&project).expect("analyze project")
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
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

fn type_names(codebase: &Codebase) -> Vec<&str> {
    codebase
        .types()
        .iter()
        .map(|declaration| declaration.name.as_str())
        .collect()
}

#[test]
fn analyzes_a_cargo_package_and_validates_relationships() {
    let project = TempProject::new("single");
    write_package(
        &project,
        "",
        "fixture-app",
        r#"
pub trait Greet {
    fn hello(&self) -> String;
}

pub struct Person {
    pub name: String,
}

impl Person {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Greet for Person {
    fn hello(&self) -> String {
        format!("hello {}", self.name)
    }
}
"#,
    );

    let codebase = project.analyze();

    assert_eq!(codebase.files().len(), 1);
    assert_eq!(codebase.modules().len(), 1);
    assert_eq!(type_names(&codebase), vec!["Greet", "Person"]);
    let person = codebase
        .types()
        .iter()
        .find(|declaration| declaration.name == "Person")
        .expect("Person declaration");
    assert_eq!(person.methods.len(), 3);
    assert_eq!(person.fields.len(), 1);
    assert_ne!(person.fields[0].id, person.methods[1].id);
    assert_eq!(codebase.relationships().len(), 1);
    let relationship = &codebase.relationships()[0];
    assert_eq!(relationship.resolution, ResolutionStatus::Inferred);
    assert!(codebase.contains_declaration(&relationship.source));
    assert!(codebase.contains_declaration(relationship.target.symbol().unwrap()));

    let metadata = codebase.analysis();
    assert_eq!(metadata.mode, AnalysisMode::SyntaxOnly);
    assert_eq!(metadata.files_analyzed, 1);
    assert_eq!(metadata.files_failed, 0);
    assert!(!metadata.partial);
    assert!(codebase.diagnostics().is_empty());
}

#[test]
fn analyzes_every_package_in_a_workspace() {
    let project = TempProject::new("workspace");
    project.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"2\"\n",
    );
    write_package(&project, "alpha", "alpha", "pub struct Alpha;\n");
    write_package(&project, "beta", "beta", "pub struct Beta;\n");

    let codebase = project.analyze();

    assert_eq!(type_names(&codebase), vec!["Alpha", "Beta"]);
    assert_eq!(codebase.files().len(), 2);
    assert_eq!(codebase.analysis().files_analyzed, 2);
    assert_eq!(codebase.analysis().files_failed, 0);
    assert!(!codebase.analysis().partial);
    assert!(codebase.diagnostics().is_empty());
}

#[test]
fn malformed_files_produce_diagnostics_without_losing_valid_declarations() {
    let project = TempProject::new("partial");
    project.write(
        "Cargo.toml",
        "[package]\nname = \"partial\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    project.write("src/lib.rs", "mod broken;\npub struct Good;\n");
    project.write("src/broken.rs", "pub struct Broken { this is not Rust\n");

    let codebase = project.analyze();

    assert_eq!(type_names(&codebase), vec!["Good"]);
    assert!(codebase.diagnostics().iter().any(|diagnostic| diagnostic
        .location
        .as_ref()
        .is_some_and(|location| location.file == Path::new("src/broken.rs"))));
    assert_eq!(codebase.analysis().files_analyzed, 1);
    assert_eq!(codebase.analysis().files_failed, 1);
    assert!(codebase.analysis().partial);
}

#[test]
fn missing_manifest_is_reported_as_an_analysis_error() {
    let project = TempProject::new("no-manifest");
    let project_path = ProjectPath::resolve(Some(project.path())).expect("resolve project");

    let error = analyze_project(&project_path).expect_err("missing manifest fails");

    assert!(matches!(
        error,
        AnalysisError::Discovery(codeuml_analysis::DiscoveryError::ManifestMissing(_))
    ));
}
