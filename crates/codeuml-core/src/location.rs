use std::path::PathBuf;

/// A source span with 1-based line and column numbers.
///
/// Paths are project-relative so locations can be shown to users without
/// leaking absolute paths from the analyst's machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl SourceLocation {
    pub fn new(
        file: impl Into<PathBuf>,
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            column,
            end_line,
            end_column,
        }
    }
}
