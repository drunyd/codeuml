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
    max_lines: usize,
) -> ArchitectureDiagram {
    if full {
        build_full(codebase, max_lines)
    } else {
        build_level(codebase, parent, max_lines)
    }
}

fn build_level(
    codebase: &Codebase,
    parent: Option<&SymbolId>,
    max_lines: usize,
) -> ArchitectureDiagram {
    let mut nodes = Vec::new();
    if let Some(module_id) = parent {
        if let Some(module) = codebase.module(module_id) {
            nodes.push(module_node(codebase, module, max_lines));
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
        nodes.push(module_node(codebase, module, max_lines));
        selectable.push(module.id.clone());
    }
    for declaration in types {
        nodes.push(type_node(codebase, declaration, max_lines));
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

fn build_full(codebase: &Codebase, max_lines: usize) -> ArchitectureDiagram {
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
        nodes.push(module_node(codebase, module, max_lines));
        selectable.push(module.id.clone());
    }
    for declaration in types {
        nodes.push(type_node(codebase, declaration, max_lines));
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

fn module_node(codebase: &Codebase, module: &Module, max_lines: usize) -> DiagramNode {
    let level = module_depth(codebase, module);
    let mut lines = vec![format!("Level {level}")];
    let mut nested: Vec<String> = codebase
        .modules()
        .iter()
        .filter(|child| child.parent.as_ref() == Some(&module.id))
        .map(|child| format!("{}/", child.name))
        .collect();
    nested.extend(
        codebase
            .types()
            .iter()
            .filter(|declaration| declaration.module.as_ref() == Some(&module.id))
            .map(|declaration| format!("{} [{}]", declaration.name, declaration.kind)),
    );
    nested.sort_by_key(|line| line.to_lowercase());
    let has_entries = !nested.is_empty();
    append_capped_lines(&mut lines, nested, max_lines);
    let extra_rule_after = has_entries.then_some(1);

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
        extra_lines: lines,
        extra_rule_after,
    }
}

fn type_node(codebase: &Codebase, declaration: &TypeDecl, max_lines: usize) -> DiagramNode {
    let level = declaration
        .module
        .as_ref()
        .and_then(|module| codebase.module(module))
        .map(|module| module_depth(codebase, module) + 1)
        .unwrap_or(0);
    let mut entries = Vec::new();
    for field in &declaration.fields {
        let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
        entries.push(format!(
            "{} {}: {}",
            visibility_marker(&field.visibility),
            field.name,
            type_name
        ));
    }
    for method in declaration
        .methods
        .iter()
        .filter(|method| method.visibility == Visibility::Public)
    {
        let parameters = method
            .parameters
            .iter()
            .map(|parameter| format!("{}: {}", parameter.name, parameter.type_name))
            .collect::<Vec<_>>()
            .join(", ");
        let return_type = method
            .return_type
            .as_ref()
            .map(|return_type| format!(" -> {return_type}"))
            .unwrap_or_default();
        entries.push(format!("+ {}({parameters}){return_type}", method.name));
    }
    let mut lines = vec![format!("Level {level}")];
    let has_entries = !entries.is_empty();
    append_capped_lines(&mut lines, entries, max_lines);
    let extra_rule_after = has_entries.then_some(1);

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
        extra_lines: lines,
        extra_rule_after,
    }
}

fn visibility_marker(visibility: &Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "+",
        Visibility::Private => "-",
        _ => "~",
    }
}

fn append_capped_lines(lines: &mut Vec<String>, entries: Vec<String>, max_lines: usize) {
    if entries.is_empty() {
        return;
    }
    let shown = entries.len().min(max_lines);
    lines.extend(entries.iter().take(shown).cloned());
    let remaining = entries.len().saturating_sub(shown);
    if remaining > 0 {
        lines.push(format!("+{remaining} more"));
    }
}

fn module_depth(codebase: &Codebase, module: &Module) -> usize {
    let mut depth = 0;
    let mut parent = module.parent.clone();
    let mut guard = codebase.modules().len() + 1;
    while let Some(id) = parent {
        if guard == 0 {
            break;
        }
        guard -= 1;
        depth += 1;
        parent = codebase
            .module(&id)
            .and_then(|parent| parent.parent.clone());
    }
    depth
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
    use codeuml_core::{
        Codebase, Field, Method, Module, SourceLocation, TypeDecl, TypeKind, Visibility,
    };

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
        let ty = SymbolId::declaration("src/models.rs", "crate_root::models::Customer");
        let mut customer = TypeDecl::new(
            ty.clone(),
            "Customer",
            "crate_root::models::Customer",
            TypeKind::Struct,
            location("src/models.rs"),
        )
        .with_module(child.clone());
        customer.fields.push(
            Field::new(
                SymbolId::field(&ty, "name"),
                "name",
                location("src/models.rs"),
            )
            .with_type("String")
            .with_visibility(Visibility::Public),
        );
        customer.fields.push(
            Field::new(
                SymbolId::field(&ty, "secret"),
                "secret",
                location("src/models.rs"),
            )
            .with_type("u32")
            .with_visibility(Visibility::Private),
        );
        customer.methods.push(
            Method::new(
                SymbolId::method(&ty, "find"),
                "find",
                location("src/models.rs"),
            )
            .with_visibility(Visibility::Public),
        );
        customer.methods.push(
            Method::new(
                SymbolId::method(&ty, "helper"),
                "helper",
                location("src/models.rs"),
            )
            .with_visibility(Visibility::Private),
        );
        codebase.add_type(customer).unwrap();
        (codebase, root, child, nested)
    }

    #[test]
    fn level_view_contains_only_the_requested_children_plus_context() {
        let (codebase, _root, child, nested) = fixture();
        let view = build_architecture_diagram(&codebase, Some(&child), false, 4);
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
        let view = build_architecture_diagram(&codebase, None, true, 4);
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
        let root_node = view
            .projection
            .nodes
            .iter()
            .find(|node| node.id == root)
            .expect("root module");
        assert!(root_node.extra_lines.iter().any(|line| line == "Level 0"));
        assert!(root_node.extra_lines.iter().any(|line| line == "models/"));
    }

    #[test]
    fn long_member_lists_are_capped_with_an_omission_count() {
        let (codebase, _root, child, _nested) = fixture();
        let view = build_architecture_diagram(&codebase, Some(&child), false, 1);
        let customer = view
            .projection
            .nodes
            .iter()
            .find(|node| node.name == "Customer")
            .expect("Customer node");
        assert!(customer.extra_lines.iter().any(|line| line == "Level 2"));
        assert!(customer.extra_lines.iter().any(|line| line == "+2 more"));
    }

    #[test]
    fn architecture_boxes_allow_eight_entries_before_summarising() {
        let mut codebase = Codebase::new("many");
        let module = SymbolId::declaration("src/lib.rs", "many");
        codebase
            .add_module(Module::new(
                module.clone(),
                "many",
                "many",
                "src/lib.rs",
                location("src/lib.rs"),
            ))
            .unwrap();
        let ty = SymbolId::declaration("src/lib.rs", "many::Many");
        let mut declaration = TypeDecl::new(
            ty.clone(),
            "Many",
            "many::Many",
            TypeKind::Struct,
            location("src/lib.rs"),
        )
        .with_module(module.clone());
        for index in 0..10 {
            declaration.fields.push(Field::new(
                SymbolId::field(&ty, &format!("field{index}")),
                format!("field{index}"),
                location("src/lib.rs"),
            ));
        }
        codebase.add_type(declaration).unwrap();

        let view = build_architecture_diagram(&codebase, Some(&module), false, 8);
        let node = view
            .projection
            .nodes
            .iter()
            .find(|node| node.name == "Many")
            .expect("Many node");
        assert_eq!(node.extra_rule_after, Some(1));
        assert!(node.extra_lines.iter().any(|line| line == "+2 more"));
        let field_lines = node
            .extra_lines
            .iter()
            .filter(|line| line.starts_with('+') || line.starts_with('-'))
            .count();
        assert_eq!(field_lines, 9, "eight fields plus the omission line");
    }

    #[test]
    fn graph_building_is_deterministic() {
        let (codebase, _root, child, _nested) = fixture();
        let first = build_architecture_diagram(&codebase, Some(&child), false, 4);
        let second = build_architecture_diagram(&codebase, Some(&child), false, 4);
        assert_eq!(first.projection, second.projection);
        assert_eq!(first.selectable, second.selectable);
    }
}
