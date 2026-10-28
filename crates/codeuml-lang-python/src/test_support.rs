//! Helpers shared by the unit tests of this crate.

use std::fs;
use std::path::PathBuf;

use crate::build::AnalyzedFile;
use crate::discovery::PySource;
use crate::extract::extract;

/// A source at a project-relative path with no source-root stripping.
pub(crate) fn py_source(path: &str) -> PySource {
    let path = PathBuf::from(path);
    let parent = path.parent().map(PathBuf::from).unwrap_or_default();
    PySource {
        stem: path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default(),
        dirs: parent
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect(),
        dir_path: parent,
        path,
    }
}

/// Parse `code` as the file at `path`.
pub(crate) fn analyzed_file(path: &str, code: &str) -> AnalyzedFile {
    let source = py_source(path);
    let facts = extract(code, &source.path).expect("valid python");
    AnalyzedFile {
        module: source.module_name(),
        source,
        facts,
    }
}

/// A throw-away directory removed on drop.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codeuml-python-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }

    /// Write `contents` to a project-relative path, creating directories.
    pub(crate) fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("directories");
        }
        fs::write(path, contents).expect("write file");
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
