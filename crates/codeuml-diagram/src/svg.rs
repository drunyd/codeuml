use std::collections::{BTreeMap, BTreeSet};

use codeuml_core::{RelationshipKind, ResolutionStatus, SymbolId, TypeKind};
use unicode_width::UnicodeWidthStr;

use crate::model::DiagramMemberKind;
use crate::{DiagramEdge, DiagramEndpoint, DiagramNode, DiagramProjection};

const MARGIN: f64 = 40.0;
const TITLE_HEIGHT: f64 = 100.0;
const NODE_PADDING: f64 = 12.0;
const NODE_TITLE_HEIGHT: f64 = 20.0;
const NODE_LINE_HEIGHT: f64 = 14.0;
const NODE_MIN_WIDTH: f64 = 220.0;
const NODE_GAP_X: f64 = 56.0;
const NODE_GAP_Y: f64 = 68.0;
const BOUNDARY_HEIGHT: f64 = 38.0;
const BOUNDARY_GAP: f64 = 20.0;
const LEGEND_LINE_HEIGHT: f64 = 16.0;
const FONT_FAMILY: &str = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";

const COLOR_TEXT: &str = "#1f2937";
const COLOR_MUTED: &str = "#475569";
const COLOR_NODE_FILL: &str = "#f8fafc";
const COLOR_NODE_STROKE: &str = "#334155";
const COLOR_BOUNDARY_FILL: &str = "#fffbeb";
const COLOR_BOUNDARY_STROKE: &str = "#b45309";
const COLOR_UNRESOLVED_FILL: &str = "#fef2f2";
const COLOR_UNRESOLVED_STROKE: &str = "#b91c1c";

const EDGE_STYLES: [(&str, &str, Option<&str>); 7] = [
    ("arrow-default", "#475569", None),
    ("arrow-method", "#64748b", Some("6 3")),
    ("arrow-implements", "#1d4ed8", None),
    ("arrow-trait", "#7c3aed", Some("4 4")),
    ("arrow-alias", "#0f766e", Some("2 3")),
    ("arrow-boundary", "#b45309", Some("6 4")),
    ("arrow-unresolved", "#b91c1c", Some("6 4")),
];

#[derive(Clone, Debug, PartialEq)]
pub struct SvgOptions {
    pub title: String,
    pub subtitle: Option<String>,
    /// Maximum member lines rendered per type node.
    pub max_members: usize,
    /// Maximum rendered node width; longer content is truncated.
    pub max_node_width: f64,
    /// Target width after which a layout layer wraps into another row.
    pub max_canvas_width: f64,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            title: "CodeUML Diagram".to_owned(),
            subtitle: None,
            max_members: 6,
            max_node_width: 500.0,
            max_canvas_width: 1600.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutNode {
    pub id: SymbolId,
    pub title: String,
    pub qualified_name: String,
    pub kind: TypeKind,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Rendered body lines below the node title.
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBoundary {
    pub endpoint: DiagramEndpoint,
    pub label: String,
    pub unresolved: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutEdgeEndpoint {
    pub endpoint: DiagramEndpoint,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub anchor: LayoutPoint,
    pub boundary: bool,
    pub unresolved: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutEdge {
    pub source: LayoutEdgeEndpoint,
    pub target: LayoutEdgeEndpoint,
    pub kind: RelationshipKind,
    pub resolution: ResolutionStatus,
    pub label: String,
    pub points: Vec<LayoutPoint>,
    pub stroke: String,
    pub dash: Option<String>,
    pub marker_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutLegend {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiagramLayout {
    pub title: String,
    pub subtitle: Option<String>,
    pub width: f64,
    pub height: f64,
    pub nodes: Vec<LayoutNode>,
    pub boundaries: Vec<LayoutBoundary>,
    pub edges: Vec<LayoutEdge>,
    pub legend: LayoutLegend,
    pub empty_message: Option<String>,
}

struct NodeContent {
    title: String,
    qualified_name: String,
    kind: TypeKind,
    lines: Vec<String>,
    width: f64,
    height: f64,
}

/// Compute a deterministic layered layout from an existing projection.
pub fn layout_projection(projection: &DiagramProjection, options: &SvgOptions) -> DiagramLayout {
    let max_node_width = options.max_node_width.max(NODE_MIN_WIDTH);
    let max_canvas_width = options.max_canvas_width.max(max_node_width + MARGIN * 2.0);

    let contents: Vec<NodeContent> = projection
        .nodes
        .iter()
        .map(|node| node_content(node, options, max_node_width))
        .collect();
    let node_index: BTreeMap<SymbolId, usize> = projection
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect();

    let levels = assign_levels(projection, &node_index);

    let mut nodes: Vec<LayoutNode> = projection
        .nodes
        .iter()
        .zip(contents)
        .map(|(projected, content)| LayoutNode {
            id: projected.id.clone(),
            title: content.title,
            qualified_name: content.qualified_name,
            kind: content.kind,
            x: 0.0,
            y: 0.0,
            width: content.width,
            height: content.height,
            lines: content.lines,
        })
        .collect();

    let mut grouped: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, level) in levels.iter().enumerate() {
        grouped.entry(*level).or_default().push(index);
    }

    let mut cursor_y = TITLE_HEIGHT;
    for level_nodes in grouped.values() {
        let mut row_y = cursor_y;
        let mut row_height = 0.0_f64;
        let mut x = MARGIN;
        for index in level_nodes {
            let node = &mut nodes[*index];
            if x > MARGIN && x + node.width + MARGIN > max_canvas_width {
                row_y += row_height + NODE_GAP_Y;
                row_height = 0.0;
                x = MARGIN;
            }
            node.x = x;
            node.y = row_y;
            row_height = row_height.max(node.height);
            x += node.width + NODE_GAP_X;
        }
        cursor_y = row_y + row_height + NODE_GAP_Y;
    }

    let node_boxes: Vec<LayoutRect> = nodes
        .iter()
        .map(|node| LayoutRect::new(node.x, node.y, node.width, node.height))
        .collect();
    let mut max_right = nodes
        .iter()
        .map(|node| node.x + node.width)
        .fold(0.0_f64, f64::max);

    let boundary_endpoints = collect_boundaries(projection);
    let mut boundaries = Vec::new();
    if !boundary_endpoints.is_empty() {
        cursor_y += BOUNDARY_GAP;
        let mut x = MARGIN;
        let mut row_height = 0.0_f64;
        for endpoint in boundary_endpoints {
            let label = boundary_label(&endpoint);
            let width = estimate_text_width(&label, 11.0) + NODE_PADDING * 2.0;
            let width = width.clamp(180.0, max_node_width);
            if x > MARGIN && x + width + MARGIN > max_canvas_width {
                cursor_y += row_height + BOUNDARY_GAP;
                row_height = 0.0;
                x = MARGIN;
            }
            boundaries.push(LayoutBoundary {
                endpoint,
                label: truncate_text(&label, width - NODE_PADDING * 2.0, 11.0),
                unresolved: false,
                x,
                y: cursor_y,
                width,
                height: BOUNDARY_HEIGHT,
            });
            row_height = row_height.max(BOUNDARY_HEIGHT);
            x += width + BOUNDARY_GAP;
        }
        cursor_y += row_height + BOUNDARY_GAP;
    }
    for boundary in &mut boundaries {
        boundary.unresolved = matches!(
            &boundary.endpoint,
            DiagramEndpoint::Boundary { symbol: None, .. }
        );
        if boundary.unresolved {
            // Keep the label explicit without changing the endpoint identity.
            boundary.label = boundary.label.replacen("boundary:", "unresolved:", 1);
        }
    }
    max_right = max_right.max(
        boundaries
            .iter()
            .map(|boundary| boundary.x + boundary.width)
            .fold(0.0_f64, f64::max),
    );

    let boundary_index: BTreeMap<DiagramEndpoint, usize> = boundaries
        .iter()
        .enumerate()
        .map(|(index, boundary)| (boundary.endpoint.clone(), index))
        .collect();

    let mut edges = Vec::new();
    for edge in &projection.edges {
        let Some(source) = edge_endpoint(
            &edge.source,
            &nodes,
            &node_index,
            &node_boxes,
            &boundaries,
            &boundary_index,
        ) else {
            continue;
        };
        let Some(mut target) = edge_endpoint(
            &edge.target,
            &nodes,
            &node_index,
            &node_boxes,
            &boundaries,
            &boundary_index,
        ) else {
            continue;
        };
        let mut source = source;
        let self_edge = !source.boundary && !target.boundary && source.endpoint == target.endpoint;
        let (source_anchor, target_anchor, points) = route_edge(&source, &target, self_edge);
        source.anchor = source_anchor;
        target.anchor = target_anchor;
        let (stroke, dash, marker_id) = edge_style(edge);
        let boundary = source.boundary || target.boundary;
        let state = if edge.resolution == ResolutionStatus::Unresolved {
            "unresolved"
        } else if boundary {
            "boundary"
        } else {
            ""
        };
        let label = if state.is_empty() {
            edge.kind.to_string()
        } else {
            format!("{} ({state})", edge.kind)
        };
        edges.push(LayoutEdge {
            source,
            target,
            kind: edge.kind,
            resolution: edge.resolution,
            label,
            points,
            stroke: stroke.to_owned(),
            dash: dash.map(ToOwned::to_owned),
            marker_id: marker_id.to_owned(),
        });
        if let Some(last) = edges.last() {
            max_right = max_right
                .max(last.label_position().0 + estimate_text_width(&last.label, 9.0) / 2.0);
        }
    }

    let legend_lines = vec![
        "Edge labels show relationship kind and direction.".to_owned(),
        "Solid: structural references; dashed: methods, traits, aliases, boundaries.".to_owned(),
        "Red dashed edges indicate unresolved targets.".to_owned(),
        "Arrowheads point from source to target.".to_owned(),
    ];
    let legend_width = legend_lines
        .iter()
        .map(|line| estimate_text_width(line, 10.0))
        .fold(320.0_f64, f64::max)
        + NODE_PADDING * 2.0;
    let legend_width = legend_width.min(max_canvas_width - MARGIN * 2.0);
    let legend_height = NODE_PADDING * 2.0 + 18.0 + legend_lines.len() as f64 * LEGEND_LINE_HEIGHT;
    cursor_y += 24.0;
    let legend = LayoutLegend {
        x: MARGIN,
        y: cursor_y,
        width: legend_width,
        height: legend_height,
        lines: legend_lines,
    };
    cursor_y += legend_height + MARGIN;

    let title_width = estimate_text_width(&options.title, 20.0);
    let subtitle_width = options
        .subtitle
        .as_ref()
        .map(|subtitle| estimate_text_width(subtitle, 12.0))
        .unwrap_or(0.0);
    let width = max_right
        .max(MARGIN * 2.0 + title_width.max(subtitle_width))
        .max(640.0)
        .max(legend.x + legend.width + MARGIN);
    let empty_message = if projection.nodes.is_empty() {
        Some("No types in the selected scope.".to_owned())
    } else {
        None
    };
    let height = cursor_y.max(TITLE_HEIGHT + 40.0);

    DiagramLayout {
        title: options.title.clone(),
        subtitle: options.subtitle.clone(),
        width,
        height,
        nodes,
        boundaries,
        edges,
        legend,
        empty_message,
    }
}

impl LayoutEdge {
    fn label_position(&self) -> (f64, f64) {
        let point = label_point(&self.points);
        (point.x, point.y - 4.0)
    }
}

#[derive(Clone, Copy)]
struct LayoutRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl LayoutRect {
    fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn center(&self) -> LayoutPoint {
        LayoutPoint {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }

    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

fn node_content(node: &DiagramNode, options: &SvgOptions, max_node_width: f64) -> NodeContent {
    let mut lines = vec![format!("{} · {}", node.kind, node.visibility)];
    if node.qualified_name != node.name {
        lines.push(node.qualified_name.clone());
    }
    if let Some(alias) = &node.alias_of {
        lines.push(format!("= {alias}"));
    }
    let shown = node.members.len().min(options.max_members);
    for member in node.members.iter().take(shown) {
        let line = match member.kind {
            DiagramMemberKind::Field => format!("field {}", member.text),
            DiagramMemberKind::Variant | DiagramMemberKind::Method => member.text.clone(),
        };
        lines.push(line);
    }
    let remaining = node.members.len().saturating_sub(shown);
    if remaining > 0 {
        lines.push(format!("+{remaining} more"));
    }

    let title_width = estimate_text_width(&node.name, 14.0);
    let body_width = lines
        .iter()
        .map(|line| estimate_text_width(line, 10.0))
        .fold(0.0_f64, f64::max);
    let width =
        (title_width.max(body_width) + NODE_PADDING * 2.0).clamp(NODE_MIN_WIDTH, max_node_width);
    let max_text_width = width - NODE_PADDING * 2.0;
    let title = truncate_text(&node.name, max_text_width, 14.0);
    let lines: Vec<String> = lines
        .into_iter()
        .map(|line| truncate_text(&line, max_text_width, 10.0))
        .collect();
    let height = NODE_PADDING * 2.0 + NODE_TITLE_HEIGHT + lines.len() as f64 * NODE_LINE_HEIGHT;

    NodeContent {
        title,
        qualified_name: node.qualified_name.clone(),
        kind: node.kind.clone(),
        lines,
        width,
        height,
    }
}

fn assign_levels(
    projection: &DiagramProjection,
    node_index: &BTreeMap<SymbolId, usize>,
) -> Vec<usize> {
    let count = projection.nodes.len();
    let mut adjacency: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); count];
    let mut indegree = vec![0usize; count];
    for edge in &projection.edges {
        let (DiagramEndpoint::Node(source), DiagramEndpoint::Node(target)) =
            (&edge.source, &edge.target)
        else {
            continue;
        };
        let (Some(&source), Some(&target)) = (node_index.get(source), node_index.get(target))
        else {
            continue;
        };
        if source == target || !adjacency[source].insert(target) {
            continue;
        }
        indegree[target] += 1;
    }

    let mut ready: BTreeSet<usize> = (0..count).filter(|index| indegree[*index] == 0).collect();
    let mut levels = vec![0usize; count];
    let mut processed = vec![false; count];
    let mut maximum_level = 0usize;
    while let Some(index) = ready.pop_first() {
        processed[index] = true;
        maximum_level = maximum_level.max(levels[index]);
        let targets: Vec<usize> = adjacency[index].iter().copied().collect();
        for target in targets {
            levels[target] = levels[target].max(levels[index] + 1);
            indegree[target] -= 1;
            if indegree[target] == 0 {
                ready.insert(target);
            }
        }
    }

    let remaining_level = maximum_level + 1;
    for (index, done) in processed.iter().enumerate() {
        if !done {
            levels[index] = remaining_level;
        }
    }
    levels
}

fn collect_boundaries(projection: &DiagramProjection) -> Vec<DiagramEndpoint> {
    let mut boundaries = BTreeSet::new();
    for edge in &projection.edges {
        for endpoint in [&edge.source, &edge.target] {
            if endpoint.is_boundary() {
                boundaries.insert(endpoint.clone());
            }
        }
    }
    boundaries.into_iter().collect()
}

fn boundary_label(endpoint: &DiagramEndpoint) -> String {
    match endpoint {
        DiagramEndpoint::Boundary { symbol: None, name } => format!("unresolved: {name}"),
        DiagramEndpoint::Boundary { name, .. } => format!("boundary: {name}"),
        DiagramEndpoint::Node(id) => format!("boundary: {id}"),
    }
}

fn edge_endpoint(
    endpoint: &DiagramEndpoint,
    nodes: &[LayoutNode],
    node_index: &BTreeMap<SymbolId, usize>,
    node_boxes: &[LayoutRect],
    boundaries: &[LayoutBoundary],
    boundary_index: &BTreeMap<DiagramEndpoint, usize>,
) -> Option<LayoutEdgeEndpoint> {
    match endpoint {
        DiagramEndpoint::Node(id) => {
            let index = *node_index.get(id)?;
            let node = &nodes[index];
            let rect = node_boxes[index];
            Some(LayoutEdgeEndpoint {
                endpoint: endpoint.clone(),
                name: node.qualified_name.clone(),
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                anchor: LayoutPoint { x: 0.0, y: 0.0 },
                boundary: false,
                unresolved: false,
            })
        }
        DiagramEndpoint::Boundary { symbol, name } => {
            let index = *boundary_index.get(endpoint)?;
            let boundary = &boundaries[index];
            Some(LayoutEdgeEndpoint {
                endpoint: endpoint.clone(),
                name: name.clone(),
                x: boundary.x,
                y: boundary.y,
                width: boundary.width,
                height: boundary.height,
                anchor: LayoutPoint { x: 0.0, y: 0.0 },
                boundary: true,
                unresolved: symbol.is_none(),
            })
        }
    }
}

fn route_edge(
    source: &LayoutEdgeEndpoint,
    target: &LayoutEdgeEndpoint,
    self_edge: bool,
) -> (LayoutPoint, LayoutPoint, Vec<LayoutPoint>) {
    let source_rect = LayoutRect::new(source.x, source.y, source.width, source.height);
    let target_rect = LayoutRect::new(target.x, target.y, target.width, target.height);
    if self_edge {
        let start = LayoutPoint {
            x: source_rect.right(),
            y: source_rect.y + 14.0,
        };
        let end = LayoutPoint {
            x: source_rect.right(),
            y: source_rect.bottom() - 14.0,
        };
        let loop_x = source_rect.right() + 34.0;
        return (
            start,
            end,
            vec![
                start,
                LayoutPoint {
                    x: loop_x,
                    y: start.y,
                },
                LayoutPoint {
                    x: loop_x,
                    y: end.y,
                },
                end,
            ],
        );
    }

    let source_center = source_rect.center();
    let target_center = target_rect.center();
    let vertical_gap =
        target_rect.y >= source_rect.bottom() || source_rect.y >= target_rect.bottom();
    let horizontal_gap =
        target_rect.x >= source_rect.right() || source_rect.x >= target_rect.right();
    let use_vertical = if vertical_gap {
        true
    } else if horizontal_gap {
        false
    } else {
        (target_center.y - source_center.y).abs() >= (target_center.x - source_center.x).abs()
    };

    if use_vertical {
        let below = target_center.y >= source_center.y;
        let start = LayoutPoint {
            x: source_center.x,
            y: if below {
                source_rect.bottom()
            } else {
                source_rect.y
            },
        };
        let end = LayoutPoint {
            x: target_center.x,
            y: if below {
                target_rect.y
            } else {
                target_rect.bottom()
            },
        };
        if (start.x - end.x).abs() < 0.01 {
            return (start, end, vec![start, end]);
        }
        let middle = (start.y + end.y) / 2.0;
        (
            start,
            end,
            vec![
                start,
                LayoutPoint {
                    x: start.x,
                    y: middle,
                },
                LayoutPoint {
                    x: end.x,
                    y: middle,
                },
                end,
            ],
        )
    } else {
        let right = target_center.x >= source_center.x;
        let start = LayoutPoint {
            x: if right {
                source_rect.right()
            } else {
                source_rect.x
            },
            y: source_center.y,
        };
        let end = LayoutPoint {
            x: if right {
                target_rect.x
            } else {
                target_rect.right()
            },
            y: target_center.y,
        };
        if (start.y - end.y).abs() < 0.01 {
            return (start, end, vec![start, end]);
        }
        let middle = (start.x + end.x) / 2.0;
        (
            start,
            end,
            vec![
                start,
                LayoutPoint {
                    x: middle,
                    y: start.y,
                },
                LayoutPoint {
                    x: middle,
                    y: end.y,
                },
                end,
            ],
        )
    }
}

fn label_point(points: &[LayoutPoint]) -> LayoutPoint {
    if points.len() == 2 {
        LayoutPoint {
            x: (points[0].x + points[1].x) / 2.0,
            y: (points[0].y + points[1].y) / 2.0,
        }
    } else {
        LayoutPoint {
            x: (points[1].x + points[2].x) / 2.0,
            y: (points[1].y + points[2].y) / 2.0,
        }
    }
}

fn edge_style(edge: &DiagramEdge) -> (&'static str, Option<&'static str>, &'static str) {
    if edge.resolution == ResolutionStatus::Unresolved {
        return (EDGE_STYLES[6].1, EDGE_STYLES[6].2, EDGE_STYLES[6].0);
    }
    if edge.source.is_boundary() || edge.target.is_boundary() {
        return (EDGE_STYLES[5].1, EDGE_STYLES[5].2, EDGE_STYLES[5].0);
    }
    match edge.kind {
        RelationshipKind::MethodParameterType | RelationshipKind::MethodReturnType => {
            (EDGE_STYLES[1].1, EDGE_STYLES[1].2, EDGE_STYLES[1].0)
        }
        RelationshipKind::Implements => (EDGE_STYLES[2].1, EDGE_STYLES[2].2, EDGE_STYLES[2].0),
        RelationshipKind::TraitBound => (EDGE_STYLES[3].1, EDGE_STYLES[3].2, EDGE_STYLES[3].0),
        RelationshipKind::Aliases => (EDGE_STYLES[4].1, EDGE_STYLES[4].2, EDGE_STYLES[4].0),
        _ => (EDGE_STYLES[0].1, EDGE_STYLES[0].2, EDGE_STYLES[0].0),
    }
}

fn estimate_text_width(text: &str, font_size: f64) -> f64 {
    UnicodeWidthStr::width(text) as f64 * font_size * 0.62
}

fn truncate_text(text: &str, max_width: f64, font_size: f64) -> String {
    if estimate_text_width(text, font_size) <= max_width {
        return text.to_owned();
    }
    let ellipsis_width = estimate_text_width("...", font_size);
    let mut output = String::new();
    let mut width = 0.0;
    for character in text.chars() {
        let character_width =
            UnicodeWidthStr::width(character.encode_utf8(&mut [0; 4])) as f64 * font_size * 0.62;
        if width + character_width + ellipsis_width > max_width {
            break;
        }
        width += character_width;
        output.push(character);
    }
    output.push_str("...");
    output
}

/// Serialize a computed layout into a standalone SVG document.
pub fn render_svg(layout: &DiagramLayout) -> String {
    let mut svg = String::new();
    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" id=\"codeuml-diagram\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" role=\"img\" aria-labelledby=\"diagram-title diagram-desc\">\n",
        format_number(layout.width),
        format_number(layout.height),
        format_number(layout.width),
        format_number(layout.height),
    ));
    svg.push_str(&format!(
        "  <title id=\"diagram-title\">{}</title>\n",
        escape_xml(&layout.title)
    ));
    let description = layout
        .subtitle
        .as_deref()
        .unwrap_or("CodeUML structural diagram");
    svg.push_str(&format!(
        "  <desc id=\"diagram-desc\">{}</desc>\n",
        escape_xml(description)
    ));
    svg.push_str(&format!(
        "  <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        format_number(layout.width),
        format_number(layout.height)
    ));
    render_defs(&mut svg);
    render_header(&mut svg, layout);
    if let Some(message) = &layout.empty_message {
        svg.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"14\" fill=\"{COLOR_MUTED}\">{}</text>\n",
            format_number(MARGIN),
            format_number(TITLE_HEIGHT + 20.0),
            FONT_FAMILY,
            escape_xml(message)
        ));
    }
    render_edges(&mut svg, layout);
    render_boundaries(&mut svg, layout);
    render_nodes(&mut svg, layout);
    render_legend(&mut svg, layout);
    svg.push_str("</svg>\n");
    svg
}

pub fn render_projection_svg(projection: &DiagramProjection, options: &SvgOptions) -> String {
    let layout = layout_projection(projection, options);
    render_svg(&layout)
}

fn render_defs(svg: &mut String) {
    svg.push_str("  <defs>\n");
    for (id, color, _) in EDGE_STYLES {
        svg.push_str(&format!(
            "    <marker id=\"{id}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"{color}\"/></marker>\n"
        ));
    }
    svg.push_str("  </defs>\n");
}

fn render_header(svg: &mut String, layout: &DiagramLayout) {
    svg.push_str(&format!(
        "  <text x=\"{}\" y=\"36\" font-family=\"{FONT_FAMILY}\" font-size=\"20\" font-weight=\"700\" fill=\"{COLOR_TEXT}\">{}</text>\n",
        format_number(MARGIN),
        escape_xml(&layout.title)
    ));
    if let Some(subtitle) = &layout.subtitle {
        svg.push_str(&format!(
            "  <text x=\"{}\" y=\"60\" font-family=\"{FONT_FAMILY}\" font-size=\"12\" fill=\"{COLOR_MUTED}\">{}</text>\n",
            format_number(MARGIN),
            escape_xml(subtitle)
        ));
    }
}

fn render_nodes(svg: &mut String, layout: &DiagramLayout) {
    svg.push_str("  <g id=\"nodes\">\n");
    for (index, node) in layout.nodes.iter().enumerate() {
        svg.push_str(&format!(
            "    <g id=\"node-{index}\" class=\"type-node\">\n      <title>{}</title>\n",
            escape_xml(&node.qualified_name)
        ));
        svg.push_str(&format!(
            "      <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\" fill=\"{COLOR_NODE_FILL}\" stroke=\"{COLOR_NODE_STROKE}\" stroke-width=\"1.3\"/>\n",
            format_number(node.x),
            format_number(node.y),
            format_number(node.width),
            format_number(node.height)
        ));
        svg.push_str(&format!(
            "      <text x=\"{}\" y=\"{}\" font-family=\"{FONT_FAMILY}\" font-size=\"14\" font-weight=\"700\" fill=\"{COLOR_TEXT}\">{}</text>\n",
            format_number(node.x + NODE_PADDING),
            format_number(node.y + NODE_PADDING + 14.0),
            escape_xml(&node.title)
        ));
        for (line_index, line) in node.lines.iter().enumerate() {
            svg.push_str(&format!(
                "      <text x=\"{}\" y=\"{}\" font-family=\"{FONT_FAMILY}\" font-size=\"10\" fill=\"{COLOR_MUTED}\">{}</text>\n",
                format_number(node.x + NODE_PADDING),
                format_number(
                    node.y + NODE_PADDING + NODE_TITLE_HEIGHT + 10.0
                        + line_index as f64 * NODE_LINE_HEIGHT
                ),
                escape_xml(line)
            ));
        }
        svg.push_str("    </g>\n");
    }
    svg.push_str("  </g>\n");
}

fn render_boundaries(svg: &mut String, layout: &DiagramLayout) {
    if layout.boundaries.is_empty() {
        return;
    }
    svg.push_str("  <g id=\"boundaries\">\n");
    for (index, boundary) in layout.boundaries.iter().enumerate() {
        let (fill, stroke) = if boundary.unresolved {
            (COLOR_UNRESOLVED_FILL, COLOR_UNRESOLVED_STROKE)
        } else {
            (COLOR_BOUNDARY_FILL, COLOR_BOUNDARY_STROKE)
        };
        svg.push_str(&format!(
            "    <g id=\"boundary-{index}\" class=\"boundary-endpoint\">\n      <title>{}</title>\n",
            escape_xml(&boundary.label)
        ));
        svg.push_str(&format!(
            "      <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"19\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1.3\" stroke-dasharray=\"5 3\"/>\n",
            format_number(boundary.x),
            format_number(boundary.y),
            format_number(boundary.width),
            format_number(boundary.height)
        ));
        svg.push_str(&format!(
            "      <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"{FONT_FAMILY}\" font-size=\"11\" fill=\"{stroke}\">{}</text>\n",
            format_number(boundary.x + boundary.width / 2.0),
            format_number(boundary.y + boundary.height / 2.0 + 4.0),
            escape_xml(&boundary.label)
        ));
        svg.push_str("    </g>\n");
    }
    svg.push_str("  </g>\n");
}

fn render_edges(svg: &mut String, layout: &DiagramLayout) {
    if layout.edges.is_empty() {
        return;
    }
    svg.push_str("  <g id=\"edges\" fill=\"none\">\n");
    for (index, edge) in layout.edges.iter().enumerate() {
        let path = edge
            .points
            .iter()
            .enumerate()
            .map(|(point_index, point)| {
                format!(
                    "{} {} {}",
                    if point_index == 0 { "M" } else { "L" },
                    format_number(point.x),
                    format_number(point.y)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        svg.push_str(&format!(
            "    <g id=\"edge-{index}\" class=\"edge kind-{}\">\n      <title>{} to {}: {}</title>\n",
            edge_kind_name(edge.kind),
            escape_xml(&edge.source.name),
            escape_xml(&edge.target.name),
            escape_xml(&edge.label)
        ));
        let dash = edge
            .dash
            .as_ref()
            .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
            .unwrap_or_default();
        svg.push_str(&format!(
            "      <path d=\"{path}\" stroke=\"{}\" stroke-width=\"1.6\"{dash} marker-end=\"url(#{})\"/>\n",
            edge.stroke, edge.marker_id
        ));
        let (label_x, label_y) = edge.label_position();
        svg.push_str(&format!(
            "      <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"{FONT_FAMILY}\" font-size=\"9\" fill=\"{}\" stroke=\"#ffffff\" stroke-width=\"3\" paint-order=\"stroke\">{}</text>\n",
            format_number(label_x),
            format_number(label_y),
            edge.stroke,
            escape_xml(&edge.label)
        ));
        svg.push_str("    </g>\n");
    }
    svg.push_str("  </g>\n");
}

fn render_legend(svg: &mut String, layout: &DiagramLayout) {
    let legend = &layout.legend;
    svg.push_str("  <g id=\"legend\">\n");
    svg.push_str(&format!(
        "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"5\" fill=\"#f8fafc\" stroke=\"#cbd5e1\"/>\n",
        format_number(legend.x),
        format_number(legend.y),
        format_number(legend.width),
        format_number(legend.height)
    ));
    svg.push_str(&format!(
        "    <text x=\"{}\" y=\"{}\" font-family=\"{FONT_FAMILY}\" font-size=\"11\" font-weight=\"700\" fill=\"{COLOR_TEXT}\">Legend</text>\n",
        format_number(legend.x + NODE_PADDING),
        format_number(legend.y + NODE_PADDING + 10.0)
    ));
    for (index, line) in legend.lines.iter().enumerate() {
        svg.push_str(&format!(
            "    <text x=\"{}\" y=\"{}\" font-family=\"{FONT_FAMILY}\" font-size=\"10\" fill=\"{COLOR_MUTED}\">{}</text>\n",
            format_number(legend.x + NODE_PADDING),
            format_number(legend.y + NODE_PADDING + 18.0 + index as f64 * LEGEND_LINE_HEIGHT),
            escape_xml(line)
        ));
    }
    svg.push_str("  </g>\n");
}

fn edge_kind_name(kind: RelationshipKind) -> &'static str {
    match kind {
        RelationshipKind::Implements => "implements",
        RelationshipKind::Extends => "extends",
        RelationshipKind::Contains => "contains",
        RelationshipKind::FieldType => "field-type",
        RelationshipKind::MethodParameterType => "parameter-type",
        RelationshipKind::MethodReturnType => "return-type",
        RelationshipKind::TraitBound => "trait-bound",
        RelationshipKind::References => "references",
        RelationshipKind::Aliases => "aliases",
        _ => "reference",
    }
}

fn format_number(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if (rounded - rounded.round()).abs() < f64::EPSILON {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}
