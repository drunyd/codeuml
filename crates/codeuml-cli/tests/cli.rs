use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("codeuml-cli-{label}-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    fn write_package(&self, name: &str, source: &str) {
        self.write(
            "Cargo.toml",
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        );
        self.write("src/lib.rs", source);
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_codeuml"))
}

#[test]
fn help_describes_the_project_path_argument() {
    let output = command().arg("--help").output().expect("run codeuml");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: codeuml"), "{stdout}");
    assert!(stdout.contains("PROJECT_PATH"), "{stdout}");
}

#[test]
fn explicit_project_directory_is_reported() {
    let dir = TempDir::new("explicit");
    dir.write_package(
        "explicit",
        "pub struct Customer;\npub struct Example { pub customer: Customer }\n",
    );
    let output = command()
        .arg(dir.path())
        .output()
        .expect("run codeuml with a project path");

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let canonical = dir.path().canonicalize().expect("canonical path");
    assert!(stdout.contains("Project root:"), "{stdout}");
    assert!(
        stdout.contains(&canonical.display().to_string()),
        "{stdout}"
    );
    assert!(stdout.contains("Source files: 1"), "{stdout}");
    assert!(stdout.contains("Types: 2"), "{stdout}");
    assert!(stdout.contains("Relationships: 1"), "{stdout}");
    assert!(stdout.contains("Diagram nodes: 2"), "{stdout}");
    assert!(stdout.contains("Diagram edges: 1"), "{stdout}");
    assert!(stdout.contains("Analysis: syntax only"), "{stdout}");
}

#[test]
fn current_directory_is_used_without_an_argument() {
    let dir = TempDir::new("default");
    dir.write_package("default-app", "pub struct Default;\n");
    let output = command()
        .current_dir(dir.path())
        .output()
        .expect("run codeuml without arguments");

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let canonical = dir.path().canonicalize().expect("canonical path");
    assert!(
        stdout.contains(&canonical.display().to_string()),
        "{stdout}"
    );
    assert!(stdout.contains("Types: 1"), "{stdout}");
}

#[test]
fn missing_manifest_fails_clearly() {
    let dir = TempDir::new("missing-manifest");

    let output = command()
        .arg(dir.path())
        .output()
        .expect("run codeuml without a Cargo manifest");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Cargo.toml"), "{stderr}");
}

#[test]
fn partial_analysis_is_reported() {
    let dir = TempDir::new("partial");
    dir.write_package("partial-app", "mod broken;\npub struct Good;\n");
    dir.write("src/broken.rs", "pub struct Broken { this is not Rust\n");

    let output = command()
        .arg(dir.path())
        .output()
        .expect("run codeuml with a malformed source");

    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Types: 1"), "{stdout}");
    assert!(stdout.contains("Diagnostics: 1"), "{stdout}");
    assert!(stdout.contains("1 failed"), "{stdout}");
    assert!(stdout.contains("partial"), "{stdout}");
}

#[test]
fn missing_project_path_fails_with_an_understandable_error() {
    let missing = std::env::temp_dir().join(format!(
        "codeuml-missing-project-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let output = command()
        .arg(&missing)
        .output()
        .expect("run codeuml with a missing path");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist"), "{stderr}");
}

#[test]
fn project_path_that_is_a_file_fails_clearly() {
    let dir = TempDir::new("file");
    let file = dir.path().join("not-a-directory.rs");
    fs::write(&file, "fn main() {}\n").expect("write fixture file");

    let output = command()
        .arg(&file)
        .output()
        .expect("run codeuml with a file path");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a directory"), "{stderr}");
}
