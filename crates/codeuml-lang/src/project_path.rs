use std::path::{Path, PathBuf};

use thiserror::Error;

/// A validated, canonical project root directory.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectPath(PathBuf);

impl ProjectPath {
    /// Resolve an explicit path, or the current working directory when `None`.
    ///
    /// The path must exist and be a directory. Canonicalizing here gives the
    /// rest of the application a single absolute root while user-facing source
    /// locations remain project-relative.
    pub fn resolve(explicit: Option<&Path>) -> Result<Self, ProjectPathError> {
        let requested = match explicit {
            Some(path) => path.to_path_buf(),
            None => std::env::current_dir().map_err(ProjectPathError::CurrentDirectory)?,
        };

        let metadata = std::fs::metadata(&requested).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ProjectPathError::NotFound(requested.clone())
            } else {
                ProjectPathError::Inspect {
                    path: requested.clone(),
                    source,
                }
            }
        })?;

        if !metadata.is_dir() {
            return Err(ProjectPathError::NotADirectory(requested));
        }

        requested
            .canonicalize()
            .map(Self)
            .map_err(|source| ProjectPathError::Canonicalize {
                path: requested,
                source,
            })
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for ProjectPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

#[derive(Debug, Error)]
pub enum ProjectPathError {
    #[error("could not determine the current directory: {0}")]
    CurrentDirectory(#[source] std::io::Error),
    #[error("project path `{}` does not exist", .0.display())]
    NotFound(PathBuf),
    #[error("project path `{}` is not a directory", .0.display())]
    NotADirectory(PathBuf),
    #[error("could not inspect project path `{}`: {source}", path.display())]
    Inspect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not canonicalize project path `{}`: {source}", path.display())]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
