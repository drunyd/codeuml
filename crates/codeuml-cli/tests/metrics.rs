use std::fs;
use std::path::PathBuf;

use codeuml_analysis::ProjectPath;
use codeuml_cli::{load_metrics, LoadedProject};
use codeuml_core::{Codebase, DiagnosticSeverity};

struct TempProject(PathBuf);

impl TempProject {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codeuml-cli-metrics-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path).expect("temp project");
        Self(path)
    }

    fn loaded(&self) -> LoadedProject {
        LoadedProject {
            path: ProjectPath::resolve(Some(&self.0)).expect("project path"),
            codebase: Codebase::new("metrics"),
        }
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn metrics_load_from_the_default_report_without_diagnostics() {
    let project = TempProject::new("ok");
    fs::create_dir_all(project.0.join(".metrics")).expect("metrics dir");
    fs::write(
        project.0.join(".metrics/crap.edn"),
        r#"{:entries [{:name "f", :namespace "p::m", :crap 7.5}]}"#,
    )
    .expect("report");

    let mut loaded = project.loaded();
    let metrics = load_metrics(&mut loaded).expect("metrics");
    assert_eq!(metrics.method_score("p::m", "f"), Some(7.5));
    assert!(loaded.codebase.diagnostics().is_empty());
}

#[test]
fn projects_without_a_report_have_no_metrics_and_no_noise() {
    let project = TempProject::new("none");
    let mut loaded = project.loaded();
    assert!(load_metrics(&mut loaded).is_none());
    assert!(loaded.codebase.diagnostics().is_empty());
}

#[test]
fn broken_metrics_files_become_warning_diagnostics() {
    let project = TempProject::new("broken");
    fs::write(
        project.0.join("codeuml.toml"),
        "[crap]\ngood_max = 9\nmedium_max = 3\n",
    )
    .expect("config");
    let mut loaded = project.loaded();
    assert!(load_metrics(&mut loaded).is_none());

    let diagnostics = loaded.codebase.diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Warning);
    assert_eq!(diagnostics[0].code.as_deref(), Some("metrics::load"));
    assert!(diagnostics[0].message.contains("codeuml.toml"));
}
