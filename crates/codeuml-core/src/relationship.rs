use std::fmt;

use crate::{SourceLocation, SymbolId};

/// Structural relationship kinds exposed by diagram and UI consumers.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationshipKind {
    Implements,
    Extends,
    Contains,
    FieldType,
    MethodParameterType,
    MethodReturnType,
    TraitBound,
    References,
    Aliases,
}

impl fmt::Display for RelationshipKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Implements => "implements",
            Self::Extends => "extends",
            Self::Contains => "contains",
            Self::FieldType => "field type",
            Self::MethodParameterType => "parameter type",
            Self::MethodReturnType => "return type",
            Self::TraitBound => "trait bound",
            Self::References => "references",
            Self::Aliases => "aliases",
        };
        formatter.write_str(text)
    }
}

/// A reference whose declaration could not be identified during analysis.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UnresolvedReference {
    pub name: String,
}

impl UnresolvedReference {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// A relationship target is either a known declaration or a preserved
/// unresolved reference.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RelationshipTarget {
    Symbol(SymbolId),
    Unresolved(UnresolvedReference),
}

impl RelationshipTarget {
    pub fn symbol(&self) -> Option<&SymbolId> {
        match self {
            Self::Symbol(id) => Some(id),
            Self::Unresolved(_) => None,
        }
    }

    pub fn unresolved(&self) -> Option<&UnresolvedReference> {
        match self {
            Self::Symbol(_) => None,
            Self::Unresolved(reference) => Some(reference),
        }
    }
}

/// How confidently a relationship target was identified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResolutionStatus {
    Unresolved,
    Inferred,
    Resolved,
}

/// The analysis technique that produced a relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provenance {
    Syntax,
    Semantic,
}

/// A source span that supports a relationship.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RelationshipEvidence {
    pub location: SourceLocation,
    pub note: Option<String>,
}

impl RelationshipEvidence {
    pub fn new(location: SourceLocation) -> Self {
        Self {
            location,
            note: None,
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// A structural relationship between declarations.
///
/// Identity is `(source, target, kind)`. Evidence and resolution status are
/// deliberately excluded so repeated observations merge into one edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relationship {
    pub source: SymbolId,
    pub target: RelationshipTarget,
    pub kind: RelationshipKind,
    pub resolution: ResolutionStatus,
    pub provenance: Provenance,
    pub evidence: Vec<RelationshipEvidence>,
}

impl Relationship {
    pub fn new(
        source: SymbolId,
        target: RelationshipTarget,
        kind: RelationshipKind,
        resolution: ResolutionStatus,
        provenance: Provenance,
    ) -> Self {
        Self {
            source,
            target,
            kind,
            resolution,
            provenance,
            evidence: Vec::new(),
        }
    }

    pub fn inferred(source: &SymbolId, target: &SymbolId, kind: RelationshipKind) -> Self {
        Self::new(
            source.clone(),
            RelationshipTarget::Symbol(target.clone()),
            kind,
            ResolutionStatus::Inferred,
            Provenance::Syntax,
        )
    }

    pub fn resolved(source: &SymbolId, target: &SymbolId, kind: RelationshipKind) -> Self {
        Self::new(
            source.clone(),
            RelationshipTarget::Symbol(target.clone()),
            kind,
            ResolutionStatus::Resolved,
            Provenance::Semantic,
        )
    }

    pub fn unresolved(
        source: &SymbolId,
        target_name: impl Into<String>,
        kind: RelationshipKind,
    ) -> Self {
        Self::new(
            source.clone(),
            RelationshipTarget::Unresolved(UnresolvedReference::new(target_name)),
            kind,
            ResolutionStatus::Unresolved,
            Provenance::Syntax,
        )
    }

    pub fn with_evidence(mut self, evidence: RelationshipEvidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self.source == other.source && self.target == other.target && self.kind == other.kind
    }

    pub(crate) fn merge_observation(&mut self, other: &Self) {
        for evidence in &other.evidence {
            if !self.evidence.contains(evidence) {
                self.evidence.push(evidence.clone());
            }
        }

        self.resolution = self.resolution.max(other.resolution);
        self.provenance = self.provenance.max(other.provenance);
    }
}
