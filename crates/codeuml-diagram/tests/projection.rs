use std::path::PathBuf;

use codeuml_core::{
    Codebase, Diagnostic, Field, Module, Relationship, RelationshipEvidence, RelationshipKind,
    ResolutionStatus, SourceLocation, SymbolId, TypeDecl, TypeKind,
};
use codeuml_diagram::{
    project, DiagramEndpoint, DiagramScope, NeighborhoodDirection, ProjectionError,
};

fn location(file: &str, line: u32) -> SourceLocation {
    SourceLocation::new(file, line, 1, line, 10)
}

fn add_module(codebase: &mut Codebase, name: &str) -> SymbolId {
    let file = format!("src/{name}.rs");
    let id = SymbolId::declaration(&file, name);
    codebase
        .add_module(Module::new(
            id.clone(),
            name,
            name,
            file,
            location(&format!("src/{name}.rs"), 1),
        ))
        .expect("module is unique");
    id
}

fn add_type(
    codebase: &mut Codebase,
    module: &SymbolId,
    file: &str,
    name: &str,
    kind: TypeKind,
    fields: &[(&str, &str)],
    line: u32,
) -> SymbolId {
    let id = SymbolId::declaration(
        file,
        &format!("{}::{name}", module.as_str().split_once('#').unwrap().1),
    );
    let mut declaration = TypeDecl::new(
        id.clone(),
        name,
        format!("{}::{name}", module.as_str().split_once('#').unwrap().1),
        kind,
        location(file, line),
    )
    .with_module(module.clone());
    for (field_name, _) in fields {
        declaration.fields.push(Field::new(
            SymbolId::field(&id, field_name),
            *field_name,
            location(file, line + 1),
        ));
    }
    codebase.add_type(declaration).expect("type is unique");
    id
}

fn field_id(codebase: &Codebase, type_id: &SymbolId, name: &str) -> SymbolId {
    codebase
        .type_by_id(type_id)
        .expect("type exists")
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing field {name}"))
        .id
        .clone()
}

fn add_field_relationship(
    codebase: &mut Codebase,
    owner: &SymbolId,
    field_name: &str,
    target: &SymbolId,
    line: u32,
) {
    let source = field_id(codebase, owner, field_name);
    codebase
        .add_relationship(
            Relationship::inferred(&source, target, RelationshipKind::FieldType)
                .with_evidence(RelationshipEvidence::new(location("src/models.rs", line))),
        )
        .expect("relationship is valid");
}

struct Fixture {
    codebase: Codebase,
    models: SymbolId,
    customer: SymbolId,
    order: SymbolId,
    service: SymbolId,
    api_request: SymbolId,
    cyclic_a: SymbolId,
    cyclic_b: SymbolId,
    cyclic_c: SymbolId,
    unrelated: SymbolId,
}

fn fixture() -> Fixture {
    let mut codebase = Codebase::new("fixture");
    let models = add_module(&mut codebase, "models");
    let api = add_module(&mut codebase, "api");
    let models_file = "src/models.rs";
    let api_file = "src/api.rs";

    let customer = add_type(
        &mut codebase,
        &models,
        models_file,
        "Customer",
        TypeKind::Struct,
        &[],
        2,
    );
    let order = add_type(
        &mut codebase,
        &models,
        models_file,
        "Order",
        TypeKind::Struct,
        &[("customer", "Customer")],
        5,
    );
    let service = add_type(
        &mut codebase,
        &models,
        models_file,
        "Service",
        TypeKind::Trait,
        &[],
        9,
    );
    let cyclic_a = add_type(
        &mut codebase,
        &models,
        models_file,
        "CyclicA",
        TypeKind::Struct,
        &[("b", "CyclicB")],
        12,
    );
    let cyclic_b = add_type(
        &mut codebase,
        &models,
        models_file,
        "CyclicB",
        TypeKind::Struct,
        &[("a", "CyclicA"), ("c", "CyclicC")],
        16,
    );
    let cyclic_c = add_type(
        &mut codebase,
        &models,
        models_file,
        "CyclicC",
        TypeKind::Struct,
        &[("related", "Unrelated")],
        21,
    );
    let api_request = add_type(
        &mut codebase,
        &api,
        api_file,
        "ApiRequest",
        TypeKind::Struct,
        &[
            ("customer", "Customer"),
            ("order", "Order"),
            ("customer_again", "Customer"),
            ("external", "ExternalThing"),
        ],
        2,
    );
    let unrelated = add_type(
        &mut codebase,
        &api,
        api_file,
        "Unrelated",
        TypeKind::Struct,
        &[],
        10,
    );

    add_field_relationship(&mut codebase, &order, "customer", &customer, 6);
    add_field_relationship(&mut codebase, &cyclic_a, "b", &cyclic_b, 13);
    add_field_relationship(&mut codebase, &cyclic_b, "a", &cyclic_a, 17);
    add_field_relationship(&mut codebase, &cyclic_b, "c", &cyclic_c, 18);
    add_field_relationship(&mut codebase, &cyclic_c, "related", &unrelated, 22);
    add_field_relationship(&mut codebase, &api_request, "customer", &customer, 3);
    add_field_relationship(&mut codebase, &api_request, "order", &order, 4);
    add_field_relationship(&mut codebase, &api_request, "customer_again", &customer, 5);
    codebase
        .add_relationship(
            Relationship::unresolved(
                &field_id(&codebase, &api_request, "external"),
                "ExternalThing",
                RelationshipKind::FieldType,
            )
            .with_evidence(RelationshipEvidence::new(location(api_file, 6))),
        )
        .expect("unresolved relationship is valid");
    codebase
        .add_relationship(
            Relationship::inferred(&api_request, &service, RelationshipKind::Implements)
                .with_evidence(RelationshipEvidence::new(location(api_file, 1))),
        )
        .expect("relationship is valid");
    codebase.add_diagnostic(Diagnostic::warning(
        "fixture warning",
        Some(location(api_file, 1)),
    ));

    Fixture {
        codebase,
        models,
        customer,
        order,
        service,
        api_request,
        cyclic_a,
        cyclic_b,
        cyclic_c,
        unrelated,
    }
}

fn node_ids(projection: &codeuml_diagram::DiagramProjection) -> Vec<&SymbolId> {
    projection.nodes.iter().map(|node| &node.id).collect()
}

fn assert_projection_integrity(projection: &codeuml_diagram::DiagramProjection) {
    let nodes: std::collections::BTreeSet<_> =
        projection.nodes.iter().map(|node| &node.id).collect();
    for edge in &projection.edges {
        for endpoint in [&edge.source, &edge.target] {
            match endpoint {
                DiagramEndpoint::Node(id) => {
                    assert!(nodes.contains(id), "node endpoint must be included");
                }
                DiagramEndpoint::Boundary {
                    symbol: Some(symbol),
                    ..
                } => {
                    assert!(
                        !nodes.contains(symbol),
                        "resolved outside endpoint must not duplicate a node"
                    );
                }
                DiagramEndpoint::Boundary { symbol: None, .. } => {}
            }
        }
    }
}

#[test]
fn project_scope_includes_all_types_and_merges_member_edges() {
    let fixture = fixture();
    let projection = project(&fixture.codebase, &DiagramScope::Project).expect("project scope");

    assert_eq!(projection.nodes.len(), 8);
    assert_eq!(projection.metadata.total_types, 8);
    assert_eq!(projection.metadata.included_types, 8);
    assert_eq!(projection.metadata.total_relationships, 10);
    assert_eq!(projection.metadata.included_relationships, 10);
    assert_eq!(projection.metadata.included_edges, 9);
    assert_eq!(projection.metadata.boundary_edges, 1);
    assert_eq!(projection.metadata.unresolved_edges, 1);
    assert_eq!(projection.diagnostics.len(), 1);
    assert!(node_ids(&projection).contains(&&fixture.api_request));
    assert!(node_ids(&projection).contains(&&fixture.unrelated));

    let customer_edges: Vec<_> = projection
        .edges
        .iter()
        .filter(|edge| {
            edge.source == DiagramEndpoint::Node(fixture.api_request.clone())
                && edge.target == DiagramEndpoint::Node(fixture.customer.clone())
                && edge.kind == RelationshipKind::FieldType
        })
        .collect();
    assert_eq!(customer_edges.len(), 1, "member edges merge at node level");
    assert_eq!(customer_edges[0].source_members.len(), 2);
    assert!(customer_edges[0].evidence.len() >= 2);

    let unresolved = projection
        .edges
        .iter()
        .find(|edge| edge.kind == RelationshipKind::FieldType && edge.target.is_boundary())
        .expect("unresolved target is a boundary edge");
    assert_eq!(
        unresolved.target,
        DiagramEndpoint::Boundary {
            symbol: None,
            name: "ExternalThing".to_owned()
        }
    );
    assert_eq!(unresolved.resolution, ResolutionStatus::Unresolved);

    for edge in &projection.edges {
        if let DiagramEndpoint::Node(id) = &edge.source {
            assert!(projection.nodes.iter().any(|node| &node.id == id));
        }
        if let DiagramEndpoint::Node(id) = &edge.target {
            assert!(projection.nodes.iter().any(|node| &node.id == id));
        }
    }

    let names: Vec<_> = projection
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "nodes are deterministically ordered");
    assert_projection_integrity(&projection);
}

#[test]
fn module_scope_keeps_members_and_represents_cross_module_edges_as_boundaries() {
    let fixture = fixture();
    let projection = project(
        &fixture.codebase,
        &DiagramScope::Module {
            module: fixture.models.clone(),
        },
    )
    .expect("module scope");

    assert_eq!(projection.nodes.len(), 6);
    assert!(node_ids(&projection).contains(&&fixture.customer));
    assert!(node_ids(&projection).contains(&&fixture.service));
    assert!(!node_ids(&projection).contains(&&fixture.api_request));

    let incoming = projection
        .edges
        .iter()
        .find(|edge| {
            edge.source
                == DiagramEndpoint::Boundary {
                    symbol: Some(fixture.api_request.clone()),
                    name: "api::ApiRequest".to_owned(),
                }
                && edge.target == DiagramEndpoint::Node(fixture.order.clone())
                && edge.kind == RelationshipKind::FieldType
        })
        .expect("cross-module incoming edge is a boundary");
    assert_eq!(incoming.resolution, ResolutionStatus::Inferred);

    let outgoing = projection
        .edges
        .iter()
        .find(|edge| {
            edge.source == DiagramEndpoint::Node(fixture.cyclic_c.clone())
                && edge.target
                    == DiagramEndpoint::Boundary {
                        symbol: Some(fixture.unrelated.clone()),
                        name: "api::Unrelated".to_owned(),
                    }
        })
        .expect("cross-module outgoing edge is a boundary");
    assert_eq!(outgoing.kind, RelationshipKind::FieldType);

    let api_customer_edges: Vec<_> = projection
        .edges
        .iter()
        .filter(|edge| {
            edge.source
                == DiagramEndpoint::Boundary {
                    symbol: Some(fixture.api_request.clone()),
                    name: "api::ApiRequest".to_owned(),
                }
                && edge.target == DiagramEndpoint::Node(fixture.customer.clone())
                && edge.kind == RelationshipKind::FieldType
        })
        .collect();
    assert_eq!(
        api_customer_edges.len(),
        1,
        "boundary edges also deduplicate"
    );
    assert_eq!(api_customer_edges[0].source_members.len(), 2);

    assert!(projection
        .edges
        .iter()
        .any(|edge| edge.kind == RelationshipKind::Implements
            && edge.target == DiagramEndpoint::Node(fixture.service.clone())));
    assert_eq!(projection.metadata.omitted_relationships, 1);
    assert!(projection.metadata.boundary_edges >= 3);
    assert_projection_integrity(&projection);
}

#[test]
fn selected_type_neighborhood_is_bounded_and_direction_aware() {
    let fixture = fixture();
    let both = project(
        &fixture.codebase,
        &DiagramScope::TypeNeighborhood {
            type_id: fixture.order.clone(),
            direction: NeighborhoodDirection::Both,
        },
    )
    .expect("neighborhood");
    assert_eq!(
        node_ids(&both),
        vec![&fixture.api_request, &fixture.customer, &fixture.order]
    );
    assert_eq!(both.edges.len(), 2);
    assert!(both.edges.iter().any(|edge| {
        edge.source == DiagramEndpoint::Node(fixture.order.clone())
            && edge.target == DiagramEndpoint::Node(fixture.customer.clone())
    }));
    assert!(both.edges.iter().any(|edge| {
        edge.source == DiagramEndpoint::Node(fixture.api_request.clone())
            && edge.target == DiagramEndpoint::Node(fixture.order.clone())
    }));

    let outgoing = project(
        &fixture.codebase,
        &DiagramScope::TypeNeighborhood {
            type_id: fixture.order.clone(),
            direction: NeighborhoodDirection::Outgoing,
        },
    )
    .expect("outgoing neighborhood");
    assert_eq!(node_ids(&outgoing), vec![&fixture.customer, &fixture.order]);
    assert_eq!(outgoing.edges.len(), 1);
    assert_projection_integrity(&outgoing);

    let incoming = project(
        &fixture.codebase,
        &DiagramScope::TypeNeighborhood {
            type_id: fixture.order.clone(),
            direction: NeighborhoodDirection::Incoming,
        },
    )
    .expect("incoming neighborhood");
    assert_eq!(
        node_ids(&incoming),
        vec![&fixture.api_request, &fixture.order]
    );
    assert_eq!(incoming.edges.len(), 1);
    assert_projection_integrity(&incoming);

    let cyclic = project(
        &fixture.codebase,
        &DiagramScope::TypeNeighborhood {
            type_id: fixture.cyclic_a.clone(),
            direction: NeighborhoodDirection::Outgoing,
        },
    )
    .expect("cyclic neighborhood");
    assert_eq!(
        node_ids(&cyclic),
        vec![&fixture.cyclic_a, &fixture.cyclic_b]
    );
    assert_eq!(cyclic.edges.len(), 1, "do not expand through neighbors");
    assert!(!node_ids(&cyclic).contains(&&fixture.cyclic_c));
    assert_projection_integrity(&cyclic);
}

#[test]
fn neighborhood_preserves_unresolved_neighbor_edges() {
    let fixture = fixture();
    let projection = project(
        &fixture.codebase,
        &DiagramScope::TypeNeighborhood {
            type_id: fixture.api_request.clone(),
            direction: NeighborhoodDirection::Outgoing,
        },
    )
    .expect("neighborhood");

    assert!(projection.edges.iter().any(|edge| {
        edge.target
            == DiagramEndpoint::Boundary {
                symbol: None,
                name: "ExternalThing".to_owned(),
            }
            && edge.resolution == ResolutionStatus::Unresolved
    }));
}

#[test]
fn ambiguous_analysis_diagnostics_are_carried_into_projection() {
    let fixture = fixture();
    let mut codebase = fixture.codebase;
    codebase.add_diagnostic(
        Diagnostic::warning("ambiguous reference `Duplicate`", None)
            .with_code("analysis::ambiguous-reference"),
    );

    let projection = project(&codebase, &DiagramScope::Project).expect("project");
    assert!(projection
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.code.as_deref() == Some("analysis::ambiguous-reference") }));
}

#[test]
fn missing_scope_targets_return_structured_errors() {
    let fixture = fixture();
    let missing = SymbolId::declaration("src/missing.rs", "missing");

    assert!(matches!(
        project(&fixture.codebase, &DiagramScope::Module { module: missing.clone() }),
        Err(ProjectionError::ModuleNotFound(id)) if id == missing
    ));
    assert!(matches!(
        project(
            &fixture.codebase,
            &DiagramScope::TypeNeighborhood {
                type_id: missing.clone(),
                direction: NeighborhoodDirection::Both,
            }
        ),
        Err(ProjectionError::TypeNotFound(id)) if id == missing
    ));
}

#[test]
fn empty_projects_and_repeated_projection_are_deterministic() {
    let empty = Codebase::new("empty");
    let projection = project(&empty, &DiagramScope::Project).expect("empty project");
    assert!(projection.nodes.is_empty());
    assert!(projection.edges.is_empty());
    assert_eq!(projection.metadata.included_types, 0);

    let fixture = fixture();
    let first = project(&fixture.codebase, &DiagramScope::Project).expect("first");
    let second = project(&fixture.codebase, &DiagramScope::Project).expect("second");
    assert_eq!(first, second);
}

#[test]
fn unresolved_reference_target_helpers_are_explicit() {
    let fixture = fixture();
    let projection = project(&fixture.codebase, &DiagramScope::Project).expect("project");
    let boundary = projection
        .edges
        .iter()
        .find(|edge| edge.target.is_boundary())
        .expect("boundary edge");
    assert_eq!(boundary.target.boundary_name(), Some("ExternalThing"));
    assert_eq!(boundary.target.boundary_symbol(), None);
    assert_eq!(
        boundary.evidence[0].location.file,
        PathBuf::from("src/api.rs")
    );
}
