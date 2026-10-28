use thiserror::Error;

use crate::SymbolId;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("duplicate declaration ID `{0}`")]
    DuplicateDeclaration(SymbolId),
    #[error("relationship source `{0}` is not a declaration in this codebase")]
    UnknownRelationshipSource(SymbolId),
    #[error("relationship target `{0}` is not a declaration in this codebase")]
    UnknownRelationshipTarget(SymbolId),
    #[error("relationship state is inconsistent: {reason}")]
    InconsistentRelationship { reason: String },
}
