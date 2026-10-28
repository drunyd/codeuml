use std::fmt;

use crate::SourceLocation;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => formatter.write_str("info"),
            Self::Warning => formatter.write_str("warning"),
            Self::Error => formatter.write_str("error"),
        }
    }
}

/// A structured analysis problem. Recoverable problems are retained in the
/// model rather than aborting the whole analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub location: Option<SourceLocation>,
    pub code: Option<String>,
}

impl Diagnostic {
    pub fn new(
        severity: DiagnosticSeverity,
        message: impl Into<String>,
        location: Option<SourceLocation>,
    ) -> Self {
        Self {
            severity,
            message: message.into(),
            location,
            code: None,
        }
    }

    pub fn error(message: impl Into<String>, location: Option<SourceLocation>) -> Self {
        Self::new(DiagnosticSeverity::Error, message, location)
    }

    pub fn warning(message: impl Into<String>, location: Option<SourceLocation>) -> Self {
        Self::new(DiagnosticSeverity::Warning, message, location)
    }

    pub fn info(message: impl Into<String>, location: Option<SourceLocation>) -> Self {
        Self::new(DiagnosticSeverity::Info, message, location)
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}
