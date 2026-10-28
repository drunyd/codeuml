use std::collections::BTreeSet;

use codeuml_core::{
    Codebase, Module, Provenance, RelationshipKind, ResolutionStatus, SymbolId, TypeDecl, TypeKind,
    Visibility,
};
use codeuml_diagram::{
    DiagramEdge, DiagramEndpoint, DiagramNode, DiagramProjection, DiagramScope, ProjectionMetadata,
};

pub(crate) struct ArchitectureDiagram {
    pub projection: DiagramProjection,
    pub selectable: Vec<SymbolId>,
}

/// Build the architecture view graph for one hierarchy level or the complete
/// module/type tree. Type nodes use their real type kind; module nodes use
/// `TypeKind::Other("module")`.
pub(crate) fn build_architecture_diagram(
    codebase: &Codebase,
    parent: Option<&SymbolId>,
    full: bool,
) -> ArchitectureDiagram {
    if full {
        build_full(codebase)
    } else {
        build_level(codebase, parent)
    }
}

fn build_level(codebase: &Codebase, parent: Option<&SymbolId>) -> ArchitectureDiagram {
    let mut nodes = Vec::new();
    if let Some(module_id) = parent {
        if let Some(module) = codebase.module(module_id) {
            nodes.push(module_node(module));
        }
    }

    let mut modules = child_modules(codebase, parent);
    modules.sort_by(|left, right| {
        (left.name.to_lowercase(), &left.id).cmp(&(right.name.to_lowercase(), &right.id))
    });
    let mut types = child_types(codebase, parent);
    types.sort_by(|left, right| {
        (left.name.to_lowercase(), &left.id).cmp(&(right.name.to_lowercase(), &right.id))
    });

    let mut selectable = Vec::new();
    for module in modules {
        nodes.push(module_node(module));
        selectable.push(module.id.clone());
    }
    for declaration in types {
        nodes.push(type_node(declaration));
        selectable.push(declaration.id.clone());
    }

    let mut edges = Vec::new();
    if let Some(parent) = parent {
        for id in &selectable {
            edges.push(contains_edge(parent.clone(), id.clone()));
        }
    }

    ArchitectureDiagram {
        projection: projection(nodes, edges),
        selectable,
    }
}

fn build_full(codebase: &Codebase) -> ArchitectureDiagram {
    let mut modules: Vec<&Module> = codebase.modules().iter().collect();
    modules.sort_by(|left, right| {
        (&left.qualified_name, &left.id).cmp(&(&right.qualified_name, &right.id))
    });
    let mut types: Vec<&TypeDecl> = codebase.types().iter().collect();
    types.sort_by(|left, right| {
        (&left.qualified_name, &left.id).cmp(&(&right.qualified_name, &right.id))
    });

    let mut nodes = Vec::new();
    let mut selectable = Vec::new();
    for module in modules {
        nodes.push(module_node(module));
        selectable.push(module.id.clone());
    }
    for declaration in types {
        nodes.push(type_node(declaration));
        selectable.push(declaration.id.clone());
    }

    let mut edges = Vec::new();
    let node_ids: BTreeSet<&SymbolId> = selectable.iter().collect();
    for module in codebase.modules() {
        if let Some(parent) = &module.parent {
            if node_ids.contains(&module.id) && node_ids.contains(parent) {
                edges.push(contains_edge(parent.clone(), module.id.clone()));
            }
        }
    }
    for declaration in codebase.types() {
        if let Some(module) = &declaration.module {
            if node_ids.contains(&declaration.id) && node_ids.contains(module) {
                edges.push(contains_edge(module.clone(), declaration.id.clone()));
            }
        }
    }
    edges.sort_by(|left, right| {
        (endpoint_key(&left.source), endpoint_key(&left.target))
            .cmp(&(endpoint_key(&right.source), endpoint_key(&right.target)))
    });

    ArchitectureDiagram {
        projection: projection(nodes, edges),
        selectable,
    }
}

fn child_modules<'a>(codebase: &'a Codebase, parent: Option<&SymbolId>) -> Vec<&'a Module> {
    codebase
        .modules()
        .iter()
        .filter(|module| module.parent.as_ref() == parent)
        .collect()
}

fn child_types<'a>(codebase: &'a Codebase, parent: Option<&SymbolId>) -> Vec<&'a TypeDecl> {
    codebase
        .types()
        .iter()
        .filter(|declaration| declaration.module.as_ref() == parent)
        .collect()
}

fn module_node(module: &Module) -> DiagramNode {
    DiagramNode {
        id: module.id.clone(),
        name: module.name.clone(),
        qualified_name: module.qualified_name.clone(),
        kind: TypeKind::Other("module".to_owned()),
        module: module.parent.clone(),
        location: module.location.clone(),
        visibility: Visibility::Public,
        alias_of: None,
        field_count: 0,
        method_count: 0,
        variant_count: 0,
        members: Vec::new(),
        extra_lines: vec![
            module.qualified_name.clone(),
            module.file.display().to_string(),
        ],
    }
}

fn type_node(declaration: &TypeDecl) -> DiagramNode {
    let mut summary = format!(
        "{} fields · {} methods",
        declaration.fields.len(),
        declaration.methods.len()
    );
    if !declaration.variants.is_empty() {
        summary.push_str(&format!(" · {} variants", declaration.variants.len()));
    }
    DiagramNode {
        id: declaration.id.clone(),
        name: declaration.name.clone(),
        qualified_name: declaration.qualified_name.clone(),
        kind: declaration.kind.clone(),
        module: declaration.module.clone(),
        location: declaration.location.clone(),
        visibility: declaration.visibility.clone(),
        alias_of: declaration.alias_of.clone(),
        field_count: declaration.fields.len(),
        method_count: declaration.methods.len(),
        variant_count: declaration.variants.len(),
        members: Vec::new(),
        extra_lines: vec![declaration.qualified_name.clone(), summary],
    }
}

fn contains_edge(source: SymbolId, target: SymbolId) -> DiagramEdge {
    DiagramEdge {
        source: DiagramEndpoint::Node(source),
        target: DiagramEndpoint::Node(target),
        kind: RelationshipKind::Contains,
        resolution: ResolutionStatus::Inferred,
        provenance: Provenance::Syntax,
        source_members: Vec::new(),
        evidence: Vec::new(),
    }
}

fn projection(nodes: Vec<DiagramNode>, edges: Vec<DiagramEdge>) -> DiagramProjection {
    let metadata = ProjectionMetadata {
        total_types: nodes.len(),
        included_types: nodes.len(),
        total_relationships: edges.len(),
        included_relationships: edges.len(),
        omitted_relationships: 0,
        included_edges: edges.len(),
        boundary_edges: 0,
        unresolved_edges: 0,
    };
    DiagramProjection {
        scope: DiagramScope::Project,
        nodes,
        edges,
        diagnostics: Vec::new(),
        metadata,
    }
}

fn endpoint_key(endpoint: &DiagramEndpoint) -> String {
    match endpoint {
        DiagramEndpoint::Node(id) => id.as_str().to_owned(),
        DiagramEndpoint::Boundary { name, .. } => name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use codeuml_core::{Codebase, Module, SourceLocation, TypeDecl, TypeKind};

    use super::*;

    fn location(file: &str) -> SourceLocation {
        SourceLocation::new(file, 1, 1, 1, 10)
    }

    fn fixture() -> (Codebase, SymbolId, SymbolId, SymbolId) {
        let mut codebase = Codebase::new("arch");
        let root = SymbolId::declaration("src/lib.rs", "crate_root");
        let child = SymbolId::declaration("src/models.rs", "models");
        let nested = SymbolId::declaration("src/models/nested.rs", "models::nested");
        codebase
            .add_module(Module::new(
                root.clone(),
                "crate_root",
                "crate_root",
                "src/lib.rs",
                location("src/lib.rs"),
            ))
            .unwrap();
        codebase
            .add_module(
                Module::new(
                    child.clone(),
                    "models",
                    "crate_root::models",
                    "src/models.rs",
                    location("src/models.rs"),
                )
                .with_parent(root.clone()),
            )
            .unwrap();
        codebase
            .add_module(
                Module::new(
                    nested.clone(),
                    "nested",
                    "crate_root::models::nested",
                    "src/models/nested.rs",
                    location("src/models/nested.rs"),
                )
                .with_parent(child.clone()),
            )
            .unwrap();
        let ty = TypeDecl::new(
            SymbolId::declaration("src/models.rs", "crate_root::models::Customer"),
            "Customer",
            "crate_root::models::Customer",
            TypeKind::Struct,
            location("src/models.rs"),
        )
        .with_module(child.clone());
        codebase.add_type(ty).unwrap();
        (codebase, root, child, nested)
    }

    #[test]
    fn level_view_contains_only_the_requested_children_plus_context() {
        let (codebase, _root, child, nested) = fixture();
        let view = build_architecture_diagram(&codebase, Some(&child), false);
        let ids: Vec<_> = view.projection.nodes.iter().map(|node| &node.id).collect();
        assert!(ids.contains(&&child));
        assert!(ids.contains(&&nested));
        assert!(view
            .projection
            .nodes
            .iter()
            .any(|node| node.name == "Customer"));
        assert_eq!(view.selectable.len(), 2);
        assert!(!view.selectable.contains(&child));
        assert_eq!(view.projection.edges.len(), 2);
        assert!(matches!(
            &view.projection.nodes[0].kind,
            TypeKind::Other(kind) if kind == "module"
        ));
    }

    #[test]
    fn full_view_contains_the_whole_hierarchy_and_containment_edges() {
        let (codebase, root, child, nested) = fixture();
        let view = build_architecture_diagram(&codebase, None, true);
        assert_eq!(view.projection.nodes.len(), 4);
        assert_eq!(view.projection.edges.len(), 3);
        assert!(view.selectable.contains(&root));
        assert!(view.selectable.contains(&child));
        assert!(view.selectable.contains(&nested));
        assert!(view
            .projection
            .edges
            .iter()
            .all(|edge| edge.kind == RelationshipKind::Contains));
    }

    #[test]
    fn graph_building_is_deterministic() {
        let (codebase, _root, child, _nested) = fixture();
        let first = build_architecture_diagram(&codebase, Some(&child), false);
        let second = build_architecture_diagram(&codebase, Some(&child), false);
        assert_eq!(first.projection, second.projection);
        assert_eq!(first.selectable, second.selectable);
    }
}
