use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct TempProject {
    path: PathBuf,
}

impl TempProject {
    fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codeuml-svg-cli-{label}-{}-{id}",
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

    fn write_project(&self, name: &str) {
        self.write(
            "Cargo.toml",
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        );
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codeuml"));
        command.arg(self.path());
        command
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn standard_project(label: &str) -> TempProject {
    let project = TempProject::new(label);
    project.write_project("svg-app");
    project.write(
        "src/lib.rs",
        r#"
pub mod api;
pub mod models;

pub use models::Customer;

pub struct Root {
    pub customer: models::Customer,
    pub api: api::Api,
}
"#,
    );
    project.write(
        "src/models.rs",
        "pub struct Customer { pub name: String }\n",
    );
    project.write("src/api.rs", "pub struct Api;\n");
    project
}

fn parse_svg(svg: &str) {
    let document = roxmltree::Document::parse(svg).expect("well-formed SVG");
    assert_eq!(document.root_element().tag_name().name(), "svg");
}

#[test]
fn svg_to_stdout_is_clean_noninteractive_and_deterministic() {
    let project = standard_project("stdout");
    let first = project
        .command()
        .args(["--format", "svg"])
        .output()
        .expect("run SVG export");
    assert!(first.status.success(), "{:?}", first);
    let stdout = String::from_utf8(first.stdout).expect("UTF-8 SVG");
    assert!(stdout.starts_with("<?xml"));
    assert!(!stdout.contains("Project root:"));
    assert!(!stdout.contains('\u{1b}'));
    parse_svg(&stdout);

    let second = project
        .command()
        .args(["--format", "svg"])
        .output()
        .expect("run SVG export again");
    assert!(second.status.success());
    assert_eq!(stdout.as_bytes(), second.stdout);
}

#[test]
fn svg_output_file_supports_no_overwrite_and_force() {
    let project = standard_project("file");
    let output = project.path().join("diagram.svg");

    let first = project
        .command()
        .args(["--format", "svg", "--output"])
        .arg(&output)
        .output()
        .expect("write SVG file");
    assert!(first.status.success(), "{:?}", first);
    let original = fs::read_to_string(&output).expect("SVG file");
    parse_svg(&original);

    let second = project
        .command()
        .args(["--format", "svg", "--output"])
        .arg(&output)
        .output()
        .expect("second SVG file write");
    assert!(!second.status.success());
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("already exists"), "{stderr}");
    assert_eq!(
        fs::read_to_string(&output).expect("unchanged SVG"),
        original
    );

    let forced = project
        .command()
        .args(["--format", "svg", "--force", "--output"])
        .arg(&output)
        .output()
        .expect("forced SVG file write");
    assert!(forced.status.success(), "{:?}", forced);
    parse_svg(&fs::read_to_string(&output).expect("forced SVG"));
}

#[test]
fn output_path_implies_svg_mode() {
    let project = standard_project("implicit");
    let output = project.path().join("implicit.svg");
    let result = project
        .command()
        .args(["--output"])
        .arg(&output)
        .output()
        .expect("implicit SVG export");
    assert!(result.status.success(), "{:?}", result);
    assert!(result.stdout.is_empty());
    parse_svg(&fs::read_to_string(output).expect("SVG output"));
}

#[test]
fn module_and_neighborhood_scopes_are_selectable_and_invalid_targets_fail() {
    let project = standard_project("scopes");
    let module_output = project.path().join("module.svg");
    let module = project
        .command()
        .args([
            "--format", "svg", "--scope", "module", "--module", "models", "--output",
        ])
        .arg(&module_output)
        .output()
        .expect("module SVG");
    assert!(module.status.success(), "{:?}", module);
    let module_svg = fs::read_to_string(&module_output).expect("module SVG");
    parse_svg(&module_svg);
    assert!(module_svg.contains("Scope: Module"));
    assert!(module_svg.contains("Customer"));

    let neighborhood_output = project.path().join("neighborhood.svg");
    let neighborhood = project
        .command()
        .args([
            "--format",
            "svg",
            "--scope",
            "neighborhood",
            "--type",
            "Customer",
            "--output",
        ])
        .arg(&neighborhood_output)
        .output()
        .expect("neighborhood SVG");
    assert!(neighborhood.status.success(), "{:?}", neighborhood);
    let neighborhood_svg = fs::read_to_string(&neighborhood_output).expect("neighborhood SVG");
    parse_svg(&neighborhood_svg);
    assert!(neighborhood_svg.contains("Scope: Neighborhood"));

    let missing_module = project
        .command()
        .args([
            "--format", "svg", "--scope", "module", "--module", "missing",
        ])
        .output()
        .expect("missing module");
    assert!(!missing_module.status.success());
    assert!(String::from_utf8_lossy(&missing_module.stderr).contains("not found"));

    let missing_type = project
        .command()
        .args([
            "--format",
            "svg",
            "--scope",
            "neighborhood",
            "--type",
            "Missing",
        ])
        .output()
        .expect("missing type");
    assert!(!missing_type.status.success());
    assert!(String::from_utf8_lossy(&missing_type.stderr).contains("not found"));

    for args in [
        vec!["--format", "svg", "--scope", "module"],
        vec!["--format", "svg", "--scope", "neighborhood"],
        vec!["--format", "summary", "--output", "x.svg"],
    ] {
        let result = project
            .command()
            .args(&args)
            .output()
            .expect("invalid CLI combination");
        assert!(!result.status.success(), "args: {args:?}");
    }
}

#[test]
fn ambiguous_short_type_names_are_reported() {
    let project = TempProject::new("ambiguous");
    project.write_project("ambiguous-app");
    project.write("src/lib.rs", "pub mod a;\npub mod b;\npub struct Same;\n");
    project.write("src/a.rs", "pub struct Same;\n");
    project.write("src/b.rs", "pub struct Same;\n");

    let result = project
        .command()
        .args(["--format", "svg", "--type", "Same"])
        .output()
        .expect("ambiguous type selector");

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("ambiguous"), "{stderr}");
    assert!(stderr.contains("Same"), "{stderr}");
}

#[test]
fn partial_analysis_diagnostics_are_reported_on_stderr_without_corrupting_svg() {
    let project = TempProject::new("partial");
    project.write_project("partial-svg");
    project.write("src/lib.rs", "pub mod broken;\npub struct Good;\n");
    project.write("src/broken.rs", "pub struct Broken { this is not Rust\n");

    let result = project
        .command()
        .args(["--format", "svg"])
        .output()
        .expect("partial SVG export");

    assert!(result.status.success(), "{:?}", result);
    let stdout = String::from_utf8(result.stdout).expect("SVG stdout");
    assert!(stdout.starts_with("<?xml"));
    parse_svg(&stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("diagnostic"), "{stderr}");
    assert!(stderr.contains("broken.rs"), "{stderr}");
}

#[test]
fn output_write_failures_are_clear() {
    let project = standard_project("write-failure");
    let output = project.path().join("missing-directory/diagram.svg");

    let result = project
        .command()
        .args(["--format", "svg", "--output"])
        .arg(&output)
        .output()
        .expect("failed SVG write");

    assert!(!result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).is_empty());
}

#[test]
fn help_advertises_export_options_and_summary_remains_available() {
    let help = Command::new(env!("CARGO_BIN_EXE_codeuml"))
        .arg("--help")
        .output()
        .expect("help");
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    for option in [
        "--format", "--output", "--scope", "--module", "--type", "--force",
    ] {
        assert!(help.contains(option), "help missing {option}: {help}");
    }

    let project = standard_project("summary");
    let summary = project
        .command()
        .args(["--format", "summary"])
        .output()
        .expect("summary");
    assert!(summary.status.success());
    let stdout = String::from_utf8_lossy(&summary.stdout);
    assert!(stdout.contains("Project root:"));
    assert!(stdout.contains("Diagram nodes:"));
    assert!(!stdout.contains("<?xml"));
}

#[test]
fn version_remains_functional() {
    let output = Command::new(env!("CARGO_BIN_EXE_codeuml"))
        .arg("--version")
        .output()
        .expect("version");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("codeuml 0.1.0"));
}

#[test]
fn force_without_a_file_output_or_summary_export_options_are_rejected() {
    let project = standard_project("usage");
    for args in [
        vec!["--force"],
        vec!["--format", "svg", "--force"],
        vec!["--format", "summary", "--scope", "project"],
        vec![
            "--format", "svg", "--scope", "project", "--module", "models",
        ],
    ] {
        let result = project
            .command()
            .args(&args)
            .output()
            .expect("invalid usage");
        assert!(!result.status.success(), "args: {args:?}");
    }
}
