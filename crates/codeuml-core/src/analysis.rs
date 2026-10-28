use std::fmt;

/// The highest analysis level that contributed to a codebase model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AnalysisMode {
    #[default]
    SyntaxOnly,
    SyntaxAndSemantic,
}

impl fmt::Display for AnalysisMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SyntaxOnly => formatter.write_str("syntax only"),
            Self::SyntaxAndSemantic => formatter.write_str("syntax + semantic"),
        }
    }
}

/// Provenance and completeness information for an analysis run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnalysisMetadata {
    pub mode: AnalysisMode,
    pub files_analyzed: usize,
    pub files_failed: usize,
    pub partial: bool,
}
