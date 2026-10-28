use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use codeuml_lang::{ProjectPath, ProjectPathError};

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codeuml-analysis-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn explicit_directory_is_canonicalized() {
    let dir = TempDir::new("canonical");

    let project = ProjectPath::resolve(Some(dir.path())).expect("directory is accepted");

    assert_eq!(
        project.as_path(),
        dir.path().canonicalize().expect("canonical path")
    );
    assert!(project.as_path().is_absolute());
}

#[test]
fn current_directory_is_used_when_no_path_is_given() {
    let project = ProjectPath::resolve(None).expect("current directory is accepted");
    let expected = std::env::current_dir()
        .expect("current directory")
        .canonicalize()
        .expect("canonical current directory");

    assert_eq!(project.as_path(), expected);
}

#[test]
fn missing_path_reports_the_requested_path() {
    let missing = std::env::temp_dir().join(format!(
        "codeuml-definitely-missing-project-{}-{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ));

    let error = ProjectPath::resolve(Some(&missing)).expect_err("missing paths are rejected");

    assert!(matches!(error, ProjectPathError::NotFound(_)));
    let message = error.to_string();
    assert!(message.contains("does not exist"), "{message}");
}

#[test]
fn file_paths_are_rejected() {
    let dir = TempDir::new("file-path");
    let file = dir.path().join("Cargo.toml");
    fs::write(&file, "[package]\n").expect("write fixture file");

    let error = ProjectPath::resolve(Some(&file)).expect_err("files are not project roots");

    assert!(matches!(error, ProjectPathError::NotADirectory(_)));
    assert!(error.to_string().contains("not a directory"));
}
