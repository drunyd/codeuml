use codeuml_core::{
    Diagnostic, Provenance, RelationshipEvidence, RelationshipKind, ResolutionStatus,
    SourceLocation, SymbolId, TypeKind, Visibility,
};

/// Which declarations and relationships a projection includes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagramScope {
    /// Every project type and every relationship whose source is a project type.
    Project,
    /// Types declared directly in one module. Relationships touching the module
    /// are retained; resolved endpoints outside it become boundary endpoints.
    Module { module: SymbolId },
    /// One type plus its direct neighbors. Neighbor expansion is never
    /// recursive.
    TypeNeighborhood {
        type_id: SymbolId,
        direction: NeighborhoodDirection,
    },
}

/// Direction policy for selected-type neighborhoods.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NeighborhoodDirection {
    Outgoing,
    Incoming,
    #[default]
    Both,
}

impl NeighborhoodDirection {
    pub fn includes_outgoing(self) -> bool {
        matches!(self, Self::Outgoing | Self::Both)
    }

    pub fn includes_incoming(self) -> bool {
        matches!(self, Self::Incoming | Self::Both)
    }
}

/// A compact member summary carried by a diagram node. Full member details
/// remain in `codeuml-core` and the interactive TUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagramMemberKind {
    Field,
    Variant,
    Method,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramMember {
    pub kind: DiagramMemberKind,
    pub text: String,
}

/// A type node in a diagram projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramNode {
    pub id: SymbolId,
    pub name: String,
    pub qualified_name: String,
    pub kind: TypeKind,
    pub module: Option<SymbolId>,
    pub location: SourceLocation,
    pub visibility: Visibility,
    pub alias_of: Option<String>,
    pub field_count: usize,
    pub method_count: usize,
    pub variant_count: usize,
    pub members: Vec<DiagramMember>,
    /// View-specific lines used by specialized renderers (for example the
    /// architecture map). Type projections normally leave this empty.
    pub extra_lines: Vec<String>,
}

/// One end of a diagram edge.
///
/// `Boundary` represents a resolved declaration outside the selected node set
/// or an unresolved reference. A boundary without a symbol is unresolved.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagramEndpoint {
    Node(SymbolId),
    Boundary {
        symbol: Option<SymbolId>,
        name: String,
    },
}

impl DiagramEndpoint {
    pub fn is_boundary(&self) -> bool {
        matches!(self, Self::Boundary { .. })
    }

    pub fn boundary_name(&self) -> Option<&str> {
        match self {
            Self::Node(_) => None,
            Self::Boundary { name, .. } => Some(name),
        }
    }

    pub fn boundary_symbol(&self) -> Option<&SymbolId> {
        match self {
            Self::Node(_) => None,
            Self::Boundary { symbol, .. } => symbol.as_ref(),
        }
    }

    pub fn node_id(&self) -> Option<&SymbolId> {
        match self {
            Self::Node(id) => Some(id),
            Self::Boundary { .. } => None,
        }
    }
}

/// A node-level edge. Multiple member-level core relationships that map to the
/// same source node, target endpoint, and kind are merged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramEdge {
    pub source: DiagramEndpoint,
    pub target: DiagramEndpoint,
    pub kind: RelationshipKind,
    pub resolution: ResolutionStatus,
    pub provenance: Provenance,
    /// Core member declarations (fields, methods, variants) that produced this
    /// node-level edge. Sorted and deduplicated.
    pub source_members: Vec<SymbolId>,
    pub evidence: Vec<RelationshipEvidence>,
}

/// Deterministic, UI-independent diagram projection over an analyzed model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramProjection {
    pub scope: DiagramScope,
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    pub diagnostics: Vec<Diagnostic>,
    pub metadata: ProjectionMetadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectionMetadata {
    pub total_types: usize,
    pub included_types: usize,
    pub total_relationships: usize,
    /// Core relationships selected by the scope before node-level deduplication.
    pub included_relationships: usize,
    pub omitted_relationships: usize,
    pub included_edges: usize,
    pub boundary_edges: usize,
    pub unresolved_edges: usize,
}
