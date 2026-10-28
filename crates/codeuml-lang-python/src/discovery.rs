//! Finding the Python sources of a project and naming their modules.
//!
//! A project is recognised by a packaging marker at its root (or, failing
//! that, Python files at the root or directly under `src/`). Sources are every
//! `.py` file below the root except virtual environments, caches, build
//! output, hidden directories, and test code. A `src/` directory that holds
//! Python is a source root, so `src/pkg/mod.py` is the module `pkg.mod`.
//! Symbolic links are never followed.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use codeuml_core::Diagnostic;

const MARKERS: &[&str] = &[
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "requirements.txt",
    "Pipfile",
];

const EXCLUDED_DIRECTORIES: &[&str] = &[
    "__pycache__",
    "node_modules",
    "site-packages",
    "build",
    "dist",
    "venv",
    "env",
    "target",
    "tests",
    "test",
    "testdata",
    "vendor",
    "out",
    "coverage",
];

const INIT_STEM: &str = "__init__";

/// One Python source file and where it sits in the module tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PySource {
    /// Project-relative path to the file.
    pub path: PathBuf,
    /// Project-relative path to the directory holding the file.
    pub dir_path: PathBuf,
    /// Package segments, outermost first, relative to the source root.
    pub dirs: Vec<String>,
    /// The file name without `.py`.
    pub stem: String,
}

impl PySource {
    /// `__init__.py` is the package itself.
    pub(crate) fn is_init(&self) -> bool {
        self.stem == INIT_STEM
    }

    /// Dotted name of the module this file defines. A project-root
    /// `__init__.py` has no package name and is called `__init__`.
    pub(crate) fn module_name(&self) -> String {
        let mut segments = self.dirs.clone();
        if !self.is_init() {
            segments.push(self.stem.clone());
        }
        if segments.is_empty() {
            return INIT_STEM.to_owned();
        }
        segments.join(".")
    }

    /// Dotted name of the package relative imports start from.
    pub(crate) fn package_name(&self) -> String {
        self.dirs.join(".")
    }
}

#[derive(Debug, Default)]
pub(crate) struct Discovered {
    pub sources: Vec<PySource>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Whether `root` looks like a Python project. This only inspects the root and
/// `src/`, never parses, and never walks the tree.
pub(crate) fn is_python_project(root: &Path) -> bool {
    MARKERS.iter().any(|marker| root.join(marker).is_file())
        || has_python_files(root)
        || has_python_files(&root.join("src"))
}

pub(crate) fn discover(root: &Path) -> Discovered {
    let roots = source_roots(root);
    let mut discovered = Discovered::default();
    walk(root, Path::new(""), &roots, &mut discovered);
    discovered
        .sources
        .sort_by(|left, right| left.path.cmp(&right.path));
    discovered
}

fn is_python_file(name: &OsStr) -> bool {
    Path::new(name).extension() == Some(OsStr::new("py"))
}

fn has_python_files(directory: &Path) -> bool {
    fs::read_dir(directory).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| is_python_file(&entry.file_name()) && is_plain_file(&entry))
    })
}

fn is_plain_file(entry: &fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|kind| kind.is_file())
}

fn is_plain_directory(entry: &fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|kind| kind.is_dir())
}

/// `src/` is a source root when it holds Python files or packages.
fn source_roots(root: &Path) -> Vec<PathBuf> {
    let src = root.join("src");
    let holds_python = has_python_files(&src)
        || fs::read_dir(&src).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                is_plain_directory(&entry) && entry.path().join("__init__.py").is_file()
            })
        });
    if holds_python {
        vec![PathBuf::from("src"), PathBuf::new()]
    } else {
        vec![PathBuf::new()]
    }
}

fn walk(root: &Path, relative: &Path, roots: &[PathBuf], found: &mut Discovered) {
    let absolute = root.join(relative);
    let entries = match fs::read_dir(&absolute) {
        Ok(entries) => entries,
        Err(error) => {
            found.diagnostics.push(
                Diagnostic::warning(
                    format!("could not read directory `{}`: {error}", relative.display()),
                    None,
                )
                .with_code("python::unreadable-directory"),
            );
            return;
        }
    };
    let mut entries: Vec<fs::DirEntry> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        visit_entry(root, relative, &entry, roots, found);
    }
}

fn visit_entry(
    root: &Path,
    relative: &Path,
    entry: &fs::DirEntry,
    roots: &[PathBuf],
    found: &mut Discovered,
) {
    let name = entry.file_name();
    let child = relative.join(&name);
    if is_plain_directory(entry) {
        if !is_excluded_directory(&name, &entry.path()) {
            walk(root, &child, roots, found);
        }
    } else if is_plain_file(entry) && is_python_file(&name) && !is_test_file(&name) {
        found.sources.extend(make_source(&child, roots));
    }
}

fn is_excluded_directory(name: &OsStr, path: &Path) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.')
        || name.ends_with(".egg-info")
        || EXCLUDED_DIRECTORIES.contains(&name.as_ref())
        || path.join("pyvenv.cfg").is_file()
}

fn is_test_file(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with("test_")
        || name.ends_with("_test.py")
        || name.ends_with("_tests.py")
        || name == "conftest.py"
}

/// Module dots would corrupt dotted names, so they become underscores.
fn sanitize(component: &OsStr) -> String {
    component.to_string_lossy().replace('.', "_")
}

fn make_source(path: &Path, roots: &[PathBuf]) -> Option<PySource> {
    let inner = roots.iter().find_map(|root| path.strip_prefix(root).ok())?;
    let stem = sanitize(inner.file_stem()?);
    let dirs = inner
        .parent()?
        .components()
        .map(|component| sanitize(component.as_os_str()))
        .collect();
    Some(PySource {
        path: path.to_path_buf(),
        dir_path: path.parent()?.to_path_buf(),
        dirs,
        stem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn layout(dir: &TempDir, files: &[&str]) {
        for file in files {
            dir.write(file, "");
        }
    }

    fn modules(found: &Discovered) -> Vec<String> {
        found.sources.iter().map(PySource::module_name).collect()
    }

    #[test]
    fn flat_projects_name_modules_from_their_directories() {
        let dir = TempDir::new("flat");
        layout(
            &dir,
            &[
                "main.py",
                "app/__init__.py",
                "app/models.py",
                "app/sub/util.py",
            ],
        );
        let found = discover(&dir.0);
        assert_eq!(
            modules(&found),
            ["app", "app.models", "app.sub.util", "main"]
        );
        let util = &found.sources[2];
        assert_eq!(util.package_name(), "app.sub");
        assert_eq!(util.dirs, ["app", "sub"]);
        assert_eq!(util.dir_path, PathBuf::from("app").join("sub"));
        assert!(found.sources[0].is_init() && !util.is_init());
        assert_eq!(found.sources[0].package_name(), "app");
    }

    #[test]
    fn a_src_directory_with_python_is_a_source_root() {
        let dir = TempDir::new("src");
        layout(
            &dir,
            &["src/pkg/__init__.py", "src/pkg/a.py", "docs/conf.py"],
        );
        let found = discover(&dir.0);
        assert_eq!(modules(&found), ["docs.conf", "pkg", "pkg.a"]);

        let plain = TempDir::new("nosrc");
        layout(&plain, &["src/readme.txt", "pkg/a.py"]);
        assert_eq!(modules(&discover(&plain.0)), ["pkg.a"]);

        let loose = TempDir::new("srcfiles");
        layout(&loose, &["src/tool.py"]);
        assert_eq!(modules(&discover(&loose.0)), ["tool"]);
    }

    #[test]
    fn environments_caches_builds_hidden_directories_and_tests_are_skipped() {
        let dir = TempDir::new("excluded");
        layout(
            &dir,
            &[
                "keep.py",
                "tests/test_a.py",
                "test/b.py",
                "test_c.py",
                "d_test.py",
                "e_tests.py",
                "conftest.py",
                "myenv/lib.py",
                ".hidden/x.py",
                "__pycache__/y.py",
                "node_modules/z.py",
                "build/b.py",
                "dist/d.py",
                "pkg.egg-info/e.py",
                "venv/v.py",
                "notes.txt",
                "stub.pyi",
            ],
        );
        dir.write("myenv/pyvenv.cfg", "home = x");
        assert_eq!(modules(&discover(&dir.0)), ["keep"]);
    }

    #[test]
    fn dots_in_names_cannot_corrupt_dotted_module_paths() {
        let dir = TempDir::new("dots");
        layout(&dir, &["my.dir/weird.name.py"]);
        let found = discover(&dir.0);
        assert_eq!(modules(&found), ["my_dir.weird_name"]);
        assert_eq!(
            found.sources[0].path,
            PathBuf::from("my.dir").join("weird.name.py")
        );
    }

    #[test]
    fn symbolic_links_are_not_followed() {
        let dir = TempDir::new("links");
        layout(&dir, &["real/a.py"]);
        let link = dir.0.join("alias");
        let created = symlink(&dir.0.join("real"), &link);
        let found = discover(&dir.0);
        if created {
            assert_eq!(modules(&found), ["real.a"]);
        }
    }

    #[cfg(windows)]
    fn symlink(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
    }

    #[cfg(not(windows))]
    fn symlink(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    #[test]
    fn unreadable_roots_become_diagnostics() {
        let dir = TempDir::new("missing");
        let found = discover(&dir.0.join("does-not-exist"));
        assert!(found.sources.is_empty());
        assert_eq!(found.diagnostics.len(), 1);
        assert_eq!(
            found.diagnostics[0].code.as_deref(),
            Some("python::unreadable-directory")
        );
    }

    #[test]
    fn projects_are_recognised_by_markers_or_python_at_the_root_or_src() {
        let empty = TempDir::new("none");
        assert!(!is_python_project(&empty.0));
        empty.write("notes.txt", "");
        assert!(!is_python_project(&empty.0));

        for marker in MARKERS {
            let dir = TempDir::new("marker");
            dir.write(marker, "");
            assert!(is_python_project(&dir.0), "{marker}");
        }

        let root_file = TempDir::new("rootpy");
        root_file.write("script.py", "");
        assert!(is_python_project(&root_file.0));

        let under_src = TempDir::new("srcpy");
        under_src.write("src/tool.py", "");
        assert!(is_python_project(&under_src.0));

        let nested_only = TempDir::new("nested");
        nested_only.write("deep/inside/tool.py", "");
        assert!(!is_python_project(&nested_only.0));
    }

    #[test]
    fn python_file_detection_is_exact() {
        assert!(is_python_file(OsStr::new("a.py")));
        assert!(!is_python_file(OsStr::new("a.pyi")));
        assert!(!is_python_file(OsStr::new("py")));
        assert!(!is_test_file(OsStr::new("testing.py")));
    }
}
