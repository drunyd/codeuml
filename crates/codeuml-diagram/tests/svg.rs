use std::collections::BTreeSet;

use codeuml_core::{
    Codebase, Field, Method, Module, Relationship, RelationshipEvidence, RelationshipKind,
    ResolutionStatus, SourceLocation, SymbolId, TypeDecl, TypeKind, Visibility,
};
use codeuml_diagram::{
    layout_projection, project, render_projection_svg, render_svg, DiagramEdge, DiagramEndpoint,
    DiagramMember, DiagramMemberKind, DiagramNode, DiagramProjection, DiagramScope,
    NeighborhoodDirection, SvgOptions,
};

fn location(file: &str, line: u32) -> SourceLocation {
    SourceLocation::new(file, line, 1, line, 20)
}

fn node(id: &str, name: &str, kind: TypeKind) -> DiagramNode {
    DiagramNode {
        id: SymbolId::declaration("src/model.rs", id),
        name: name.to_owned(),
        qualified_name: format!("fixture::{id}"),
        kind,
        module: None,
        location: location("src/model.rs", 1),
        field_count: 0,
        method_count: 0,
        variant_count: 0,
        visibility: Visibility::Public,
        alias_of: None,
        members: Vec::new(),
        extra_lines: Vec::new(),
    }
}

fn member(kind: DiagramMemberKind, text: &str) -> DiagramMember {
    DiagramMember {
        kind,
        text: text.to_owned(),
    }
}

fn edge(source: &DiagramNode, target: &DiagramNode, kind: RelationshipKind) -> DiagramEdge {
    DiagramEdge {
        source: DiagramEndpoint::Node(source.id.clone()),
        target: DiagramEndpoint::Node(target.id.clone()),
        kind,
        resolution: ResolutionStatus::Inferred,
        provenance: codeuml_core::Provenance::Syntax,
        source_members: Vec::new(),
        evidence: Vec::new(),
    }
}

fn projection(nodes: Vec<DiagramNode>, edges: Vec<DiagramEdge>) -> DiagramProjection {
    DiagramProjection {
        scope: DiagramScope::Project,
        nodes,
        edges,
        diagnostics: Vec::new(),
        metadata: Default::default(),
    }
}

fn parse(svg: &str) -> roxmltree::Document<'_> {
    roxmltree::Document::parse(svg).expect("well-formed SVG")
}

fn rects_overlap(left: &codeuml_diagram::LayoutNode, right: &codeuml_diagram::LayoutNode) -> bool {
    left.x < right.x + right.width
        && right.x < left.x + left.width
        && left.y < right.y + right.height
        && right.y < left.y + left.height
}

#[test]
fn produces_valid_standalone_svg_with_title_and_viewbox() {
    let first = node("A", "Alpha", TypeKind::Struct);
    let second = node("B", "Beta", TypeKind::Enum);
    let projection = projection(
        vec![first.clone(), second.clone()],
        vec![edge(&first, &second, RelationshipKind::FieldType)],
    );
    let svg = render_projection_svg(
        &projection,
        &SvgOptions {
            title: "Fixture project".to_owned(),
            subtitle: Some("Scope: Project".to_owned()),
            ..SvgOptions::default()
        },
    );
    let document = parse(&svg);

    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    let root = document.root_element();
    assert_eq!(root.tag_name().name(), "svg");
    assert_eq!(
        root.tag_name().namespace(),
        Some("http://www.w3.org/2000/svg")
    );
    assert!(root.attribute("width").is_some());
    assert!(root.attribute("height").is_some());
    let width = root.attribute("width").expect("width");
    let height = root.attribute("height").expect("height");
    assert_eq!(
        root.attribute("viewBox"),
        Some(format!("0 0 {width} {height}").as_str())
    );
    assert!(root.attribute("aria-labelledby").is_some());
    assert!(svg.contains("Fixture project"));
    assert!(svg.contains("Scope: Project"));
    assert!(svg.contains("<title"));
    assert!(svg.contains("<desc"));
    assert!(svg.contains("Alpha"));
    assert!(svg.contains("Beta"));
}

#[test]
fn xml_escaping_and_unicode_survive_parsing() {
    let mut special = node(
        "A",
        "A & B <C> \"q\" 's'",
        TypeKind::Other("type & kind".to_owned()),
    );
    special.qualified_name = "crate::A & B <C> \"q\" 's'".to_owned();
    special
        .members
        .push(member(DiagramMemberKind::Field, "value: Option<Café & δ>"));
    let unicode = node("U", "Café", TypeKind::Struct);
    let resolved = node("R", "Resolved", TypeKind::Struct);
    let mut unresolved = edge(&special, &unicode, RelationshipKind::FieldType);
    unresolved.target = DiagramEndpoint::Boundary {
        symbol: None,
        name: "Missing & <External>".to_owned(),
    };
    let projection = projection(
        vec![special, unicode.clone(), resolved.clone()],
        vec![
            unresolved,
            edge(&unicode, &resolved, RelationshipKind::Implements),
        ],
    );

    let svg = render_projection_svg(&projection, &SvgOptions::default());
    let document = parse(&svg);

    assert!(!svg.contains("<script"));
    assert!(document
        .descendants()
        .filter_map(|node| node.text())
        .any(|text| text.contains("A & B <C> \"q\" 's'")));
    assert!(document
        .descendants()
        .filter_map(|node| node.text())
        .any(|text| text.contains("Missing & <External>")));
    assert!(document
        .descendants()
        .filter_map(|node| node.text())
        .any(|text| text.contains("Café")));
    assert!(svg.contains("&amp;"));
    assert!(svg.contains("&lt;"));
    assert!(svg.contains("&gt;"));
}

#[test]
fn rendering_is_byte_deterministic() {
    let first = node("A", "Alpha", TypeKind::Struct);
    let second = node("B", "Beta", TypeKind::Struct);
    let projection = projection(
        vec![first.clone(), second.clone()],
        vec![
            edge(&first, &second, RelationshipKind::FieldType),
            edge(&second, &first, RelationshipKind::MethodReturnType),
        ],
    );
    let options = SvgOptions {
        title: "Deterministic".to_owned(),
        ..SvgOptions::default()
    };

    let first_svg = render_projection_svg(&projection, &options);
    let second_svg = render_projection_svg(&projection, &options);
    assert_eq!(first_svg, second_svg);
}

#[test]
fn layout_nodes_are_distinct_non_overlapping_and_inside_bounds() {
    let nodes: Vec<_> = (0..12)
        .map(|index| {
            node(
                &format!("N{index}"),
                &format!("Node{index}"),
                TypeKind::Struct,
            )
        })
        .collect();
    let edges: Vec<_> = nodes
        .windows(2)
        .map(|pair| edge(&pair[0], &pair[1], RelationshipKind::FieldType))
        .collect();
    let projection = projection(nodes, edges);
    let layout = layout_projection(&projection, &SvgOptions::default());

    assert_eq!(layout.nodes.len(), 12);
    for (index, left) in layout.nodes.iter().enumerate() {
        assert!(left.x >= 0.0 && left.y >= 0.0);
        assert!(left.x + left.width <= layout.width);
        assert!(left.y + left.height <= layout.height);
        for right in layout.nodes.iter().skip(index + 1) {
            assert!(
                !rects_overlap(left, right),
                "nodes overlap: {} and {}",
                left.title,
                right.title
            );
        }
    }
    for edge in &layout.edges {
        for point in &edge.points {
            assert!(point.x >= 0.0 && point.x <= layout.width);
            assert!(point.y >= 0.0 && point.y <= layout.height);
        }
    }
}

#[test]
fn resolved_edges_have_boundary_anchors_arrowheads_and_kind_labels() {
    let first = node("A", "Alpha", TypeKind::Struct);
    let second = node("B", "Beta", TypeKind::Struct);
    let kinds = [
        RelationshipKind::FieldType,
        RelationshipKind::MethodParameterType,
        RelationshipKind::MethodReturnType,
        RelationshipKind::Aliases,
        RelationshipKind::TraitBound,
        RelationshipKind::Implements,
        RelationshipKind::Extends,
        RelationshipKind::Contains,
        RelationshipKind::References,
        RelationshipKind::Aliases,
    ];
    let projection = projection(
        vec![first.clone(), second.clone()],
        kinds
            .iter()
            .map(|kind| edge(&first, &second, *kind))
            .collect(),
    );
    let layout = layout_projection(&projection, &SvgOptions::default());
    let svg = render_svg(&layout);

    assert_eq!(layout.edges.len(), kinds.len());
    for edge in &layout.edges {
        assert_eq!(edge.points.first(), Some(&edge.source.anchor));
        assert_eq!(edge.points.last(), Some(&edge.target.anchor));
        assert!(edge.marker_id.starts_with("arrow-"));
        assert!(!edge.label.is_empty());
        let source = layout
            .nodes
            .iter()
            .find(|node| node.id == first.id)
            .unwrap();
        let target = layout
            .nodes
            .iter()
            .find(|node| node.id == second.id)
            .unwrap();
        assert_on_border(edge.source.anchor, source);
        assert_on_border(edge.target.anchor, target);
    }
    let document = parse(&svg);
    let text_values: BTreeSet<_> = document
        .descendants()
        .filter(|node| node.has_tag_name("text"))
        .filter_map(|node| node.text())
        .collect();
    for kind in kinds {
        assert!(
            text_values.contains(kind.to_string().as_str()),
            "SVG should label relationship kind {kind} in a text element"
        );
    }
    assert!(svg.contains("marker-end=\"url(#arrow-"));
    assert!(svg.contains("<marker "));
    assert!(svg.contains("implements"));
}

#[test]
fn unresolved_and_boundary_endpoints_are_explicit_and_distinct() {
    let first = node("A", "Alpha", TypeKind::Struct);
    let second = node("B", "Beta", TypeKind::Struct);
    let mut unresolved_edge = edge(&first, &second, RelationshipKind::FieldType);
    unresolved_edge.target = DiagramEndpoint::Boundary {
        symbol: None,
        name: "ExternalThing".to_owned(),
    };
    unresolved_edge.resolution = ResolutionStatus::Unresolved;
    let mut boundary_edge = edge(&first, &second, RelationshipKind::TraitBound);
    boundary_edge.target = DiagramEndpoint::Boundary {
        symbol: Some(second.id.clone()),
        name: "api::Beta".to_owned(),
    };
    let projection = projection(vec![first, second], vec![unresolved_edge, boundary_edge]);
    let layout = layout_projection(&projection, &SvgOptions::default());
    let svg = render_svg(&layout);

    assert_eq!(layout.boundaries.len(), 2);
    assert!(layout
        .boundaries
        .iter()
        .any(|boundary| boundary.unresolved && boundary.label.contains("ExternalThing")));
    assert!(layout
        .boundaries
        .iter()
        .any(|boundary| !boundary.unresolved && boundary.label.contains("api::Beta")));
    assert!(svg.contains("unresolved: ExternalThing"));
    assert!(svg.contains("boundary: api::Beta"));
    assert!(svg.contains("stroke-dasharray"));
    assert!(!svg.contains("id=\"node-2\""));
}

#[test]
fn empty_single_node_disconnected_and_cyclic_projects_are_safe() {
    let empty = projection(Vec::new(), Vec::new());
    let empty_layout = layout_projection(&empty, &SvgOptions::default());
    let empty_svg = render_svg(&empty_layout);
    parse(&empty_svg);
    assert!(empty_layout.nodes.is_empty());
    assert!(empty_svg.contains("No types in the selected scope"));

    let single = node("A", "Alpha", TypeKind::Struct);
    let single_projection = projection(vec![single.clone()], Vec::new());
    let single_layout = layout_projection(&single_projection, &SvgOptions::default());
    assert_eq!(single_layout.nodes.len(), 1);
    parse(&render_svg(&single_layout));

    let a = node("A", "A", TypeKind::Struct);
    let b = node("B", "B", TypeKind::Struct);
    let c = node("C", "C", TypeKind::Struct);
    let disconnected = projection(vec![a.clone(), b.clone(), c.clone()], Vec::new());
    let disconnected_layout = layout_projection(&disconnected, &SvgOptions::default());
    assert_eq!(disconnected_layout.nodes.len(), 3);
    assert!(disconnected_layout.edges.is_empty());

    let cyclic_projection = projection(
        vec![a.clone(), b.clone()],
        vec![
            edge(&a, &b, RelationshipKind::FieldType),
            edge(&b, &a, RelationshipKind::FieldType),
        ],
    );
    let cyclic_layout = layout_projection(&cyclic_projection, &SvgOptions::default());
    assert_eq!(cyclic_layout.edges.len(), 2);
    assert_eq!(cyclic_layout.nodes.len(), 2);
}

#[test]
fn long_names_and_member_lists_are_bounded_and_marked() {
    let mut large = node(
        "Large",
        "AVeryLongTypeNameThatShouldBeTruncatedInsideTheNode",
        TypeKind::Struct,
    );
    large.qualified_name =
        "crate::deeply::nested::module::AVeryLongTypeNameThatShouldBeTruncatedInsideTheNode"
            .to_owned();
    for index in 0..20 {
        large.members.push(member(
            DiagramMemberKind::Field,
            &format!("field_with_a_long_name_{index}: SomeVeryLongTypeName<{index}>"),
        ));
    }
    let projection = projection(vec![large], Vec::new());
    let options = SvgOptions {
        max_members: 3,
        max_node_width: 360.0,
        ..SvgOptions::default()
    };
    let layout = layout_projection(&projection, &options);
    let svg = render_svg(&layout);

    assert!(layout.nodes[0].width <= 360.0);
    assert!(layout.nodes[0]
        .lines
        .iter()
        .any(|line| line.contains("+17 more")));
    assert!(svg.contains("+17 more"));
    parse(&svg);
}

#[test]
fn large_synthetic_graphs_complete_without_recursion() {
    let nodes: Vec<_> = (0..300)
        .map(|index| {
            node(
                &format!("N{index}"),
                &format!("Node{index}"),
                TypeKind::Struct,
            )
        })
        .collect();
    let edges: Vec<_> = nodes
        .windows(2)
        .map(|pair| edge(&pair[0], &pair[1], RelationshipKind::FieldType))
        .collect();
    let projection = projection(nodes, edges);
    let layout = layout_projection(
        &projection,
        &SvgOptions {
            max_canvas_width: 1200.0,
            ..SvgOptions::default()
        },
    );

    assert_eq!(layout.nodes.len(), 300);
    assert_eq!(layout.edges.len(), 299);
    assert!(layout.width > 0.0);
    assert!(layout.height > 0.0);
    parse(&render_svg(&layout));
}

#[test]
fn cross_module_boundary_rendering_and_projection_members_use_the_model() {
    let mut codebase = Codebase::new("boundary-project");
    let first_module = SymbolId::declaration("src/a.rs", "a");
    let second_module = SymbolId::declaration("src/b.rs", "b");
    codebase
        .add_module(Module::new(
            first_module.clone(),
            "a",
            "a",
            "src/a.rs",
            location("src/a.rs", 1),
        ))
        .unwrap();
    codebase
        .add_module(Module::new(
            second_module.clone(),
            "b",
            "b",
            "src/b.rs",
            location("src/b.rs", 1),
        ))
        .unwrap();

    let first = SymbolId::declaration("src/a.rs", "a::A");
    let mut first_decl = TypeDecl::new(
        first.clone(),
        "A",
        "a::A",
        TypeKind::Struct,
        location("src/a.rs", 2),
    )
    .with_module(first_module)
    .with_visibility(Visibility::Public);
    let field_id = SymbolId::field(&first, "b");
    first_decl.fields.push(
        Field::new(field_id.clone(), "b", location("src/a.rs", 3))
            .with_type("B")
            .with_visibility(Visibility::Public),
    );
    let method_id = SymbolId::method(&first, "read");
    first_decl.methods.push(
        Method::new(method_id, "read", location("src/a.rs", 5))
            .with_return_type("u64")
            .with_visibility(Visibility::Public),
    );
    codebase.add_type(first_decl).unwrap();

    let second = SymbolId::declaration("src/b.rs", "b::B");
    codebase
        .add_type(
            TypeDecl::new(
                second.clone(),
                "B",
                "b::B",
                TypeKind::Struct,
                location("src/b.rs", 2),
            )
            .with_module(second_module)
            .with_visibility(Visibility::Public),
        )
        .unwrap();
    codebase
        .add_relationship(
            Relationship::inferred(&field_id, &second, RelationshipKind::FieldType)
                .with_evidence(RelationshipEvidence::new(location("src/a.rs", 3))),
        )
        .unwrap();

    let module_scope = project(
        &codebase,
        &DiagramScope::Module {
            module: SymbolId::declaration("src/a.rs", "a"),
        },
    )
    .unwrap();
    assert_eq!(module_scope.nodes.len(), 1);
    assert!(!module_scope.nodes[0].members.is_empty());
    assert_eq!(
        module_scope.nodes[0].members[0].kind,
        DiagramMemberKind::Field
    );

    let layout = layout_projection(
        &module_scope,
        &SvgOptions {
            subtitle: Some("Scope: Module: a".to_owned()),
            ..SvgOptions::default()
        },
    );
    let svg = render_svg(&layout);
    assert_eq!(layout.boundaries.len(), 1);
    assert!(svg.contains("boundary: b::B"));
    assert!(svg.contains("Scope: Module: a"));
    parse(&svg);
}

#[test]
fn scope_labels_and_neighborhood_scope_are_reflected_in_svg() {
    let a = node("A", "Alpha", TypeKind::Struct);
    let b = node("B", "Beta", TypeKind::Struct);
    let projection = DiagramProjection {
        scope: DiagramScope::TypeNeighborhood {
            type_id: a.id.clone(),
            direction: NeighborhoodDirection::Both,
        },
        nodes: vec![a.clone(), b],
        edges: vec![edge(
            &a,
            &node("B", "Beta", TypeKind::Struct),
            RelationshipKind::FieldType,
        )],
        diagnostics: Vec::new(),
        metadata: Default::default(),
    };
    let svg = render_projection_svg(
        &projection,
        &SvgOptions {
            title: "CodeUML".to_owned(),
            subtitle: Some("Scope: Neighborhood: Alpha (both directions)".to_owned()),
            ..SvgOptions::default()
        },
    );
    assert!(svg.contains("Scope: Neighborhood: Alpha (both directions)"));
    parse(&svg);
}

fn assert_on_border(point: codeuml_diagram::LayoutPoint, node: &codeuml_diagram::LayoutNode) {
    let tolerance = 0.01;
    let on_vertical =
        (point.x - node.x).abs() < tolerance || (point.x - (node.x + node.width)).abs() < tolerance;
    let on_horizontal = (point.y - node.y).abs() < tolerance
        || (point.y - (node.y + node.height)).abs() < tolerance;
    assert!(
        on_vertical && point.y >= node.y - tolerance && point.y <= node.y + node.height + tolerance
            || on_horizontal
                && point.x >= node.x - tolerance
                && point.x <= node.x + node.width + tolerance
    );
}

#[test]
fn layout_has_unique_identifiers_for_nodes_and_markers() {
    let a = node("A", "A", TypeKind::Struct);
    let b = node("B", "B", TypeKind::Struct);
    let projection = projection(
        vec![a.clone(), b.clone()],
        vec![edge(&a, &b, RelationshipKind::Implements)],
    );
    let layout = layout_projection(&projection, &SvgOptions::default());
    let ids: BTreeSet<_> = layout.nodes.iter().map(|node| node.id.clone()).collect();
    assert_eq!(ids.len(), layout.nodes.len());
    let svg = render_svg(&layout);
    let doc = parse(&svg);
    let ids: Vec<_> = doc
        .descendants()
        .filter_map(|node| node.attribute("id"))
        .collect();
    let unique: BTreeSet<_> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len());
}

#[test]
fn renders_self_edges_and_all_relationship_kind_names() {
    let source = node("Self", "Self", TypeKind::Struct);
    let mut self_edge = edge(&source, &source, RelationshipKind::Contains);
    self_edge.source = DiagramEndpoint::Node(source.id.clone());
    self_edge.target = DiagramEndpoint::Node(source.id.clone());
    let kinds = [
        RelationshipKind::Implements,
        RelationshipKind::Extends,
        RelationshipKind::Contains,
        RelationshipKind::FieldType,
        RelationshipKind::MethodParameterType,
        RelationshipKind::MethodReturnType,
        RelationshipKind::TraitBound,
        RelationshipKind::References,
        RelationshipKind::Aliases,
    ];
    let mut edges = vec![self_edge];
    edges.extend(kinds.into_iter().map(|kind| edge(&source, &source, kind)));
    let layout = layout_projection(&projection(vec![source], edges), &SvgOptions::default());
    assert_eq!(layout.edges.len(), 10);
    assert!(layout.edges.iter().all(|edge| edge.points.len() >= 2));
    let rendered = render_svg(&layout);
    for kind in kinds {
        assert!(rendered.contains(kind.to_string().as_str()));
    }
}
