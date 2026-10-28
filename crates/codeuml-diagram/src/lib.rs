//! UI-independent diagram projection for CodeUML.
//!
//! The diagram layer reads the analyzed core model and produces purpose-built
//! type nodes and node-level edges. It does not parse source, render terminals,
//! or depend on the TUI.
//!
//! Scope policy:
//! - `Project`: all project types and all relationships from project declarations.
//! - `Module`: types declared directly in the module. Relationships with either
//!   endpoint in that module are retained; resolved endpoints outside the module
//!   become boundary endpoints rather than implicitly adding nodes.
//! - `TypeNeighborhood`: the selected type plus one hop of direct neighbors in
//!   the chosen direction. Traversal never expands through neighbors.
//!
//! Unresolved core targets are retained as boundary edges with no symbol.

mod model;
mod projection;
mod svg;

pub use model::{
    DiagramEdge, DiagramEndpoint, DiagramMember, DiagramMemberKind, DiagramNode, DiagramProjection,
    DiagramScope, NeighborhoodDirection, ProjectionMetadata,
};
pub use projection::{project, ProjectionError};
pub use svg::{
    layout_projection, render_projection_svg, render_svg, DiagramLayout, LayoutBoundary,
    LayoutEdge, LayoutEdgeEndpoint, LayoutLegend, LayoutNode, LayoutPoint, SvgOptions,
};
