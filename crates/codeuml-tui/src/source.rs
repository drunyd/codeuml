use std::fs;
use std::path::{Component, Path};

use codeuml_core::SourceLocation;

const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
const TAB_WIDTH: &str = "    ";

/// A project source file prepared for display with a highlighted span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceView {
    pub title: String,
    pub file: String,
    pub lines: Vec<String>,
    /// Zero-based inclusive line range of the highlighted declaration/evidence.
    pub highlight: (usize, usize),
}

/// Read a project-relative file and mark the span described by `location`.
///
/// Paths must stay inside the project root, including after symlink
/// resolution, so analysis data can never make the viewer read other files.
pub(crate) fn load_source(
    root: &Path,
    location: &SourceLocation,
    title: String,
) -> Result<SourceView, String> {
    let relative = &location.file;
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "Refusing to open non-project-relative path `{}`.",
            relative.display()
        ));
    }

    let root = root
        .canonicalize()
        .map_err(|error| format!("Project root is unavailable: {error}"))?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("Cannot open `{}`: {error}", relative.display()))?;
    if !path.starts_with(&root) {
        return Err(format!(
            "Refusing to open `{}` outside the project root.",
            relative.display()
        ));
    }

    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Cannot inspect `{}`: {error}", relative.display()))?;
    if !metadata.is_file() {
        return Err(format!("`{}` is not a file.", relative.display()));
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "`{}` is larger than {} KiB; not opening it.",
            relative.display(),
            MAX_SOURCE_BYTES / 1024
        ));
    }

    let text = fs::read_to_string(&path)
        .map_err(|error| format!("Cannot read `{}`: {error}", relative.display()))?;
    let lines: Vec<String> = text
        .lines()
        .map(|line| line.replace('\t', TAB_WIDTH))
        .collect();

    let last = lines.len().saturating_sub(1);
    let start = (location.line.max(1) as usize - 1).min(last);
    let end = (location.end_line.max(location.line).max(1) as usize - 1).clamp(start, last);

    Ok(SourceView {
        title,
        file: relative.display().to_string(),
        lines,
        highlight: (start, end),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn loads_file_and_clamps_the_highlighted_span() {
        let dir = TempDir::new("load");
        fs::create_dir_all(dir.0.join("src")).expect("src");
        fs::write(dir.0.join("src/lib.rs"), "struct A;\n\tstruct B;\n").expect("write");

        let view = load_source(
            &dir.0,
            &SourceLocation::new("src/lib.rs", 2, 1, 40, 1),
            "B".to_owned(),
        )
        .expect("view");
        assert_eq!(view.lines, vec!["struct A;", "    struct B;"]);
        assert_eq!(view.highlight, (1, 1));
        assert_eq!(view.file, PathBuf::from("src/lib.rs").display().to_string());
    }

    #[test]
    fn rejects_paths_that_escape_the_project_root() {
        let dir = TempDir::new("escape");
        fs::write(dir.0.join("inside.rs"), "").expect("write");

        for path in ["../outside.rs", "", "/absolute.rs"] {
            let error = load_source(
                &dir.0,
                &SourceLocation::new(path, 1, 1, 1, 1),
                "x".to_owned(),
            )
            .expect_err("rejected");
            assert!(error.contains("Refusing"), "{path}: {error}");
        }
        let error = load_source(
            &dir.0,
            &SourceLocation::new("missing.rs", 1, 1, 1, 1),
            "x".to_owned(),
        )
        .expect_err("missing");
        assert!(error.contains("Cannot open"), "{error}");
    }

    #[test]
    fn empty_files_do_not_panic() {
        let dir = TempDir::new("empty");
        fs::write(dir.0.join("empty.rs"), "").expect("write");
        let view = load_source(
            &dir.0,
            &SourceLocation::new("empty.rs", 5, 1, 5, 1),
            "x".to_owned(),
        )
        .expect("view");
        assert!(view.lines.is_empty());
        assert_eq!(view.highlight, (0, 0));
    }
}
