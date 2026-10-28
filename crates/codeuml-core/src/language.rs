use std::fmt;

/// Source language of a file or declaration.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    Other(String),
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rust => formatter.write_str("Rust"),
            Self::Python => formatter.write_str("Python"),
            Self::Other(name) => formatter.write_str(name),
        }
    }
}
