use std::collections::{BTreeMap, BTreeSet};

use codeuml_core::{RelationshipKind, ResolutionStatus, SymbolId, TypeKind};
use codeuml_diagram::{DiagramEndpoint, DiagramMemberKind, DiagramProjection};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const MARGIN_X: u16 = 2;
const MARGIN_Y: u16 = 2;
const NODE_MIN_WIDTH: u16 = 16;
const NODE_GAP_X: u16 = 4;
const NODE_GAP_Y: u16 = 3;
const BOUNDARY_GAP: u16 = 3;
const BOUNDARY_HEIGHT: u16 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiagramLayoutOptions {
    pub max_members: usize,
    pub max_node_width: u16,
    pub max_canvas_width: u16,
}

impl Default for DiagramLayoutOptions {
    fn default() -> Self {
        Self {
            max_members: 4,
            max_node_width: 38,
            max_canvas_width: 200,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiagramRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl DiagramRect {
    pub(crate) fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub(crate) fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    fn intersects(self, other: Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiagramCellPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalDiagramNode {
    pub id: SymbolId,
    pub name: String,
    pub kind: TypeKind,
    pub rect: DiagramRect,
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalDiagramBoundary {
    pub endpoint: DiagramEndpoint,
    pub rect: DiagramRect,
    pub label: String,
    pub unresolved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EdgeVisual {
    Default,
    Method,
    Implements,
    Trait,
    Alias,
    Boundary,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalDiagramEdge {
    pub source: DiagramEndpoint,
    pub target: DiagramEndpoint,
    pub kind: RelationshipKind,
    pub resolution: ResolutionStatus,
    pub visual: EdgeVisual,
    pub points: Vec<DiagramCellPoint>,
    pub label: String,
    pub label_position: Option<(u16, u16)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalDiagramLayout {
    pub width: u16,
    pub height: u16,
    pub nodes: Vec<TerminalDiagramNode>,
    pub boundaries: Vec<TerminalDiagramBoundary>,
    pub edges: Vec<TerminalDiagramEdge>,
}

impl Default for TerminalDiagramLayout {
    fn default() -> Self {
        Self {
            width: 1,
            height: 1,
            nodes: Vec::new(),
            boundaries: Vec::new(),
            edges: Vec::new(),
        }
    }
}

impl TerminalDiagramLayout {
    pub(crate) fn node_rect(&self, id: &SymbolId) -> Option<DiagramRect> {
        self.nodes
            .iter()
            .find(|node| &node.id == id)
            .map(|node| node.rect)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalDiagramStyle {
    Blank,
    NodeBorder,
    NodeText,
    NodeSelectedBorder,
    NodeSelectedText,
    Boundary,
    Unresolved,
    EdgeDefault,
    EdgeMethod,
    EdgeImplements,
    EdgeTrait,
    EdgeAlias,
    EdgeBoundary,
    EdgeUnresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCanvasCell {
    pub ch: char,
    pub style: TerminalDiagramStyle,
    pub continuation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCanvas {
    pub width: u16,
    pub height: u16,
    cells: Vec<TerminalCanvasCell>,
    line_masks: Vec<u8>,
    line_styles: Vec<TerminalDiagramStyle>,
}

impl Default for TerminalCanvas {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl TerminalCanvas {
    fn new(width: u16, height: u16) -> Self {
        let size = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            cells: vec![
                TerminalCanvasCell {
                    ch: ' ',
                    style: TerminalDiagramStyle::Blank,
                    continuation: false,
                };
                size
            ],
            line_masks: vec![0; size],
            line_styles: vec![TerminalDiagramStyle::EdgeDefault; size],
        }
    }

    pub(crate) fn cell(&self, x: u16, y: u16) -> Option<&TerminalCanvasCell> {
        self.index(x, y).map(|index| &self.cells[index])
    }

    #[cfg(test)]
    pub(crate) fn to_text(&self) -> String {
        let mut text = String::new();
        for y in 0..self.height {
            for x in 0..self.width {
                if let Some(cell) = self.cell(x, y) {
                    if !cell.continuation {
                        text.push(cell.ch);
                    }
                }
            }
            text.push('\n');
        }
        text
    }

    fn index(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(usize::from(y) * usize::from(self.width) + usize::from(x))
    }

    fn put_char(&mut self, x: u16, y: u16, ch: char, style: TerminalDiagramStyle) {
        if let Some(index) = self.index(x, y) {
            self.cells[index] = TerminalCanvasCell {
                ch,
                style,
                continuation: false,
            };
        }
    }

    fn put_text(&mut self, x: u16, y: u16, text: &str, style: TerminalDiagramStyle) {
        let mut cursor = x;
        for character in text.chars() {
            let width = UnicodeWidthChar::width(character).unwrap_or(0);
            if width == 0 {
                continue;
            }
            if usize::from(cursor) + width > usize::from(self.width) {
                break;
            }
            self.put_char(cursor, y, character, style);
            if width == 2 {
                if let Some(index) = self.index(cursor + 1, y) {
                    self.cells[index] = TerminalCanvasCell {
                        ch: ' ',
                        style,
                        continuation: true,
                    };
                }
            }
            cursor += width as u16;
        }
    }

    fn fill_rect(&mut self, rect: DiagramRect, style: TerminalDiagramStyle) {
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                if let Some(index) = self.index(x, y) {
                    self.cells[index] = TerminalCanvasCell {
                        ch: ' ',
                        style,
                        continuation: false,
                    };
                }
            }
        }
    }

    fn add_segment(&mut self, start: DiagramCellPoint, end: DiagramCellPoint, visual: EdgeVisual) {
        if start.x == end.x {
            let first = start.y.min(end.y);
            let last = start.y.max(end.y);
            for y in first..=last {
                let mut mask = 0;
                if y > first {
                    mask |= direction_mask(if start.y <= end.y {
                        Direction::Up
                    } else {
                        Direction::Down
                    });
                }
                if y < last {
                    mask |= direction_mask(if start.y <= end.y {
                        Direction::Down
                    } else {
                        Direction::Up
                    });
                }
                self.add_mask(start.x, y, mask, visual);
            }
        } else if start.y == end.y {
            let first = start.x.min(end.x);
            let last = start.x.max(end.x);
            for x in first..=last {
                let mut mask = 0;
                if x > first {
                    mask |= direction_mask(if start.x <= end.x {
                        Direction::Left
                    } else {
                        Direction::Right
                    });
                }
                if x < last {
                    mask |= direction_mask(if start.x <= end.x {
                        Direction::Right
                    } else {
                        Direction::Left
                    });
                }
                self.add_mask(x, start.y, mask, visual);
            }
        }
    }

    fn add_mask(&mut self, x: u16, y: u16, mask: u8, visual: EdgeVisual) {
        if let Some(index) = self.index(x, y) {
            self.line_masks[index] |= mask;
            self.line_styles[index] = style_for_visual(visual);
        }
    }

    fn finalize_edges(&mut self) {
        for index in 0..self.cells.len() {
            let mask = self.line_masks[index];
            if mask == 0 {
                continue;
            }
            self.cells[index].ch = line_glyph(mask);
            self.cells[index].style = self.line_styles[index];
            self.cells[index].continuation = false;
        }
    }

    fn put_arrow(&mut self, x: u16, y: u16, direction: Direction, visual: EdgeVisual) {
        let ch = match direction {
            Direction::Up => '^',
            Direction::Down => 'v',
            Direction::Left => '<',
            Direction::Right => '>',
        };
        self.put_char(x, y, ch, style_for_visual(visual));
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

fn direction_mask(direction: Direction) -> u8 {
    match direction {
        Direction::Up => 1,
        Direction::Down => 2,
        Direction::Left => 4,
        Direction::Right => 8,
    }
}

fn line_glyph(mask: u8) -> char {
    match mask {
        0 => ' ',
        1 => '╵',
        2 => '╷',
        4 => '╴',
        8 => '╶',
        3 => '│',
        12 => '─',
        10 => '┌',
        6 => '┐',
        9 => '└',
        5 => '┘',
        11 => '├',
        7 => '┤',
        14 => '┬',
        13 => '┴',
        15 => '┼',
        _ => '·',
    }
}

fn style_for_visual(visual: EdgeVisual) -> TerminalDiagramStyle {
    match visual {
        EdgeVisual::Default => TerminalDiagramStyle::EdgeDefault,
        EdgeVisual::Method => TerminalDiagramStyle::EdgeMethod,
        EdgeVisual::Implements => TerminalDiagramStyle::EdgeImplements,
        EdgeVisual::Trait => TerminalDiagramStyle::EdgeTrait,
        EdgeVisual::Alias => TerminalDiagramStyle::EdgeAlias,
        EdgeVisual::Boundary => TerminalDiagramStyle::EdgeBoundary,
        EdgeVisual::Unresolved => TerminalDiagramStyle::EdgeUnresolved,
    }
}

struct NodeContent {
    title: String,
    lines: Vec<String>,
    width: u16,
    height: u16,
}

pub(crate) fn layout_terminal_diagram(
    projection: &DiagramProjection,
    options: &DiagramLayoutOptions,
) -> TerminalDiagramLayout {
    let max_node_width = options.max_node_width.max(NODE_MIN_WIDTH);
    let max_canvas_width = options
        .max_canvas_width
        .max(max_node_width + NODE_GAP_X * 2);
    let mut nodes_spec: Vec<(&codeuml_diagram::DiagramNode, NodeContent)> = projection
        .nodes
        .iter()
        .map(|node| (node, terminal_node_content(node, options, max_node_width)))
        .collect();

    let node_index: BTreeMap<SymbolId, usize> = projection
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect();
    let levels = assign_levels(projection, &node_index);
    let mut grouped: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (index, level) in levels.iter().enumerate() {
        grouped.entry(*level).or_default().push(index);
    }

    let mut rects = vec![
        DiagramRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0
        };
        projection.nodes.len()
    ];
    let mut cursor_y = MARGIN_Y;
    for level_nodes in grouped.values() {
        let mut row_y = cursor_y;
        let mut row_height = 0u16;
        let mut x = MARGIN_X;
        for index in level_nodes {
            let spec = &mut nodes_spec[*index].1;
            if x > MARGIN_X && x + spec.width + MARGIN_X > max_canvas_width {
                row_y = row_y.saturating_add(row_height).saturating_add(NODE_GAP_Y);
                row_height = 0;
                x = MARGIN_X;
            }
            rects[*index] = DiagramRect {
                x,
                y: row_y,
                width: spec.width,
                height: spec.height,
            };
            row_height = row_height.max(spec.height);
            x = x.saturating_add(spec.width).saturating_add(NODE_GAP_X);
        }
        cursor_y = row_y.saturating_add(row_height).saturating_add(NODE_GAP_Y);
    }

    let mut nodes: Vec<TerminalDiagramNode> = Vec::new();
    for (index, (projected, content)) in nodes_spec.into_iter().enumerate() {
        nodes.push(TerminalDiagramNode {
            id: projected.id.clone(),
            name: projected.name.clone(),
            kind: projected.kind.clone(),
            rect: rects[index],
            title: content.title,
            lines: content.lines,
        });
    }

    let mut max_right = nodes
        .iter()
        .map(|node| node.rect.right())
        .max()
        .unwrap_or(0);
    let boundary_endpoints = collect_boundaries(projection);
    let mut boundaries = Vec::new();
    if !boundary_endpoints.is_empty() {
        cursor_y = cursor_y.saturating_add(BOUNDARY_GAP);
        let mut x = MARGIN_X;
        let mut row_height = 0u16;
        for endpoint in boundary_endpoints {
            let unresolved = matches!(&endpoint, DiagramEndpoint::Boundary { symbol: None, .. });
            let raw_label = match (&endpoint, unresolved) {
                (DiagramEndpoint::Boundary { name, .. }, true) => format!("unresolved: {name}"),
                (DiagramEndpoint::Boundary { name, .. }, false) => format!("boundary: {name}"),
                (DiagramEndpoint::Node(id), _) => format!("boundary: {id}"),
            };
            let width = (UnicodeWidthStr::width(raw_label.as_str()) as u16 + 4)
                .clamp(NODE_MIN_WIDTH, max_node_width);
            let label = truncate_text(&raw_label, width.saturating_sub(4));
            if x > MARGIN_X && x + width + MARGIN_X > max_canvas_width {
                cursor_y = cursor_y
                    .saturating_add(row_height)
                    .saturating_add(BOUNDARY_GAP);
                row_height = 0;
                x = MARGIN_X;
            }
            boundaries.push(TerminalDiagramBoundary {
                endpoint,
                rect: DiagramRect {
                    x,
                    y: cursor_y,
                    width,
                    height: BOUNDARY_HEIGHT,
                },
                label,
                unresolved,
            });
            row_height = row_height.max(BOUNDARY_HEIGHT);
            x = x.saturating_add(width).saturating_add(BOUNDARY_GAP);
            max_right = max_right.max(x.saturating_sub(BOUNDARY_GAP));
        }
        cursor_y = cursor_y.saturating_add(row_height);
    }

    let node_rect_by_id: BTreeMap<SymbolId, DiagramRect> = nodes
        .iter()
        .map(|node| (node.id.clone(), node.rect))
        .collect();
    let boundary_rect_by_endpoint: BTreeMap<DiagramEndpoint, DiagramRect> = boundaries
        .iter()
        .map(|boundary| (boundary.endpoint.clone(), boundary.rect))
        .collect();
    let mut pair_lanes: BTreeMap<(DiagramEndpoint, DiagramEndpoint), usize> = BTreeMap::new();
    let mut edges = Vec::new();
    for edge in &projection.edges {
        let (Some(source), Some(target)) = (
            endpoint_rect(&edge.source, &node_rect_by_id, &boundary_rect_by_endpoint),
            endpoint_rect(&edge.target, &node_rect_by_id, &boundary_rect_by_endpoint),
        ) else {
            continue;
        };
        let lane = pair_lanes
            .entry((edge.source.clone(), edge.target.clone()))
            .or_insert(0);
        let lane_offset = *lane;
        *lane += 1;
        let (points, visual) = route_edge(
            &edge.source,
            &edge.target,
            source,
            target,
            lane_offset,
            edge,
        );
        max_right = max_right.max(points.iter().map(|point| point.x + 1).max().unwrap_or(0));
        if !points.is_empty() {
            cursor_y = cursor_y.max(points.iter().map(|point| point.y + 1).max().unwrap_or(0));
        }
        edges.push(TerminalDiagramEdge {
            source: edge.source.clone(),
            target: edge.target.clone(),
            kind: edge.kind,
            resolution: edge.resolution,
            visual,
            points,
            label: edge_label(edge),
            label_position: None,
        });
    }

    let mut occupied_labels: Vec<DiagramRect> = Vec::new();
    let all_rects: Vec<DiagramRect> = nodes
        .iter()
        .map(|node| node.rect)
        .chain(boundaries.iter().map(|boundary| boundary.rect))
        .collect();
    for edge in &mut edges {
        if let Some((position, label_rect)) =
            place_label(&edge.points, &edge.label, &all_rects, &occupied_labels)
        {
            edge.label_position = Some(position);
            occupied_labels.push(label_rect);
            max_right = max_right.max(label_rect.right());
            cursor_y = cursor_y.max(label_rect.bottom());
        }
    }

    let width = max_right.saturating_add(MARGIN_X).max(24);
    let height = cursor_y.saturating_add(MARGIN_Y).max(8);
    TerminalDiagramLayout {
        width,
        height,
        nodes,
        boundaries,
        edges,
    }
}

fn terminal_node_content(
    node: &codeuml_diagram::DiagramNode,
    options: &DiagramLayoutOptions,
    max_node_width: u16,
) -> NodeContent {
    let title = format!("{} [{} {}]", node.name, node.visibility, node.kind);
    let mut lines = Vec::new();
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

    let width = (UnicodeWidthStr::width(title.as_str()) as u16)
        .max(
            lines
                .iter()
                .map(|line| UnicodeWidthStr::width(line.as_str()) as u16)
                .max()
                .unwrap_or(0),
        )
        .saturating_add(4)
        .clamp(NODE_MIN_WIDTH, max_node_width);
    let text_width = width.saturating_sub(4);
    let title = truncate_text(&title, text_width);
    let lines: Vec<String> = lines
        .into_iter()
        .map(|line| truncate_text(&line, text_width))
        .collect();
    let height = 3u16.saturating_add(lines.len() as u16);
    NodeContent {
        title,
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
    let fallback = maximum_level + 1;
    for (index, done) in processed.iter().enumerate() {
        if !done {
            levels[index] = fallback;
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

fn endpoint_rect(
    endpoint: &DiagramEndpoint,
    nodes: &BTreeMap<SymbolId, DiagramRect>,
    boundaries: &BTreeMap<DiagramEndpoint, DiagramRect>,
) -> Option<DiagramRect> {
    match endpoint {
        DiagramEndpoint::Node(id) => nodes.get(id).copied(),
        DiagramEndpoint::Boundary { .. } => boundaries.get(endpoint).copied(),
    }
}

fn route_edge(
    source_endpoint: &DiagramEndpoint,
    target_endpoint: &DiagramEndpoint,
    source: DiagramRect,
    target: DiagramRect,
    lane: usize,
    edge: &codeuml_diagram::DiagramEdge,
) -> (Vec<DiagramCellPoint>, EdgeVisual) {
    let visual = edge_visual(edge);
    if source_endpoint == target_endpoint && !source_endpoint.is_boundary() {
        let start = DiagramCellPoint {
            x: source.right().saturating_sub(1),
            y: source.y.saturating_add(1),
        };
        let end = DiagramCellPoint {
            x: source.right().saturating_sub(1),
            y: source.bottom().saturating_sub(2),
        };
        let loop_x = source.right().saturating_add(2 + lane as u16 * 2);
        return (
            vec![
                start,
                DiagramCellPoint {
                    x: loop_x,
                    y: start.y,
                },
                DiagramCellPoint {
                    x: loop_x,
                    y: end.y,
                },
                end,
            ],
            visual,
        );
    }

    let source_center_x = source.x + source.width / 2;
    let source_center_y = source.y + source.height / 2;
    let target_center_x = target.x + target.width / 2;
    let target_center_y = target.y + target.height / 2;
    let vertical_gap = target.y >= source.bottom() || source.y >= target.bottom();
    let horizontal_gap = target.x >= source.right() || source.x >= target.right();
    let use_vertical = if vertical_gap {
        true
    } else if horizontal_gap {
        false
    } else {
        target_center_y.abs_diff(source_center_y) >= target_center_x.abs_diff(source_center_x)
    };

    if use_vertical {
        let below = target_center_y >= source_center_y;
        let start = DiagramCellPoint {
            x: source_center_x,
            y: if below {
                source.bottom()
            } else {
                source.y.saturating_sub(1)
            },
        };
        let end = DiagramCellPoint {
            x: target_center_x,
            y: if below {
                target.y.saturating_sub(1)
            } else {
                target.bottom()
            },
        };
        let low = start.y.min(end.y).saturating_add(1);
        let high = start.y.max(end.y).saturating_sub(1);
        let mut middle = start.y.saturating_add(end.y) / 2;
        if lane > 0 {
            middle = middle.saturating_add(lane as u16).clamp(low, high.max(low));
        }
        let points = if start.x == end.x {
            if lane == 0 || high <= low {
                vec![start, end]
            } else {
                let bend = start.x.saturating_add(2 + lane as u16);
                vec![
                    start,
                    DiagramCellPoint { x: bend, y: low },
                    DiagramCellPoint { x: bend, y: high },
                    end,
                ]
            }
        } else {
            vec![
                start,
                DiagramCellPoint {
                    x: start.x,
                    y: middle,
                },
                DiagramCellPoint {
                    x: end.x,
                    y: middle,
                },
                end,
            ]
        };
        (points, visual)
    } else {
        let right = target_center_x >= source_center_x;
        let start = DiagramCellPoint {
            x: if right {
                source.right()
            } else {
                source.x.saturating_sub(1)
            },
            y: source_center_y,
        };
        let end = DiagramCellPoint {
            x: if right {
                target.x.saturating_sub(1)
            } else {
                target.right()
            },
            y: target_center_y,
        };
        let low = start.x.min(end.x).saturating_add(1);
        let high = start.x.max(end.x).saturating_sub(1);
        let mut middle = start.x.saturating_add(end.x) / 2;
        if lane > 0 {
            middle = middle.saturating_add(lane as u16).clamp(low, high.max(low));
        }
        let points = if start.y == end.y {
            if lane == 0 || high <= low {
                vec![start, end]
            } else {
                let bend = start.y.saturating_add(2 + lane as u16);
                vec![
                    start,
                    DiagramCellPoint { x: low, y: bend },
                    DiagramCellPoint { x: high, y: bend },
                    end,
                ]
            }
        } else {
            vec![
                start,
                DiagramCellPoint {
                    x: middle,
                    y: start.y,
                },
                DiagramCellPoint {
                    x: middle,
                    y: end.y,
                },
                end,
            ]
        };
        (points, visual)
    }
}

fn edge_visual(edge: &codeuml_diagram::DiagramEdge) -> EdgeVisual {
    if edge.resolution == ResolutionStatus::Unresolved {
        return EdgeVisual::Unresolved;
    }
    if edge.source.is_boundary() || edge.target.is_boundary() {
        return EdgeVisual::Boundary;
    }
    match edge.kind {
        RelationshipKind::MethodParameterType | RelationshipKind::MethodReturnType => {
            EdgeVisual::Method
        }
        RelationshipKind::Implements => EdgeVisual::Implements,
        RelationshipKind::TraitBound => EdgeVisual::Trait,
        RelationshipKind::Aliases => EdgeVisual::Alias,
        _ => EdgeVisual::Default,
    }
}

fn edge_label(edge: &codeuml_diagram::DiagramEdge) -> String {
    let state = if edge.resolution == ResolutionStatus::Unresolved {
        " unresolved"
    } else if edge.source.is_boundary() || edge.target.is_boundary() {
        " boundary"
    } else {
        ""
    };
    truncate_text(&format!("{}{state}", edge.kind), 18)
}

fn place_label(
    points: &[DiagramCellPoint],
    label: &str,
    blocked: &[DiagramRect],
    existing: &[DiagramRect],
) -> Option<((u16, u16), DiagramRect)> {
    let width = UnicodeWidthStr::width(label).max(1) as u16;
    for pair in points.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start.y == end.y {
            let x = start.x.min(end.x).saturating_add(1);
            let y = start.y.saturating_sub(1);
            let rect = DiagramRect {
                x,
                y,
                width,
                height: 1,
            };
            if x > 0
                && y > 0
                && blocked.iter().all(|blocked| !blocked.intersects(rect))
                && existing.iter().all(|label| !label.intersects(rect))
            {
                return Some(((x, y), rect));
            }
        } else if start.x == end.x {
            let x = start.x.saturating_add(1);
            let y = start
                .y
                .min(end.y)
                .saturating_add(start.y.abs_diff(end.y) / 2);
            let rect = DiagramRect {
                x,
                y,
                width,
                height: 1,
            };
            if x > 0
                && y > 0
                && blocked.iter().all(|blocked| !blocked.intersects(rect))
                && existing.iter().all(|label| !label.intersects(rect))
            {
                return Some(((x, y), rect));
            }
        }
    }

    let middle = points.get(points.len() / 2)?;
    let rect = DiagramRect {
        x: middle.x.saturating_add(1),
        y: middle.y,
        width,
        height: 1,
    };
    if blocked.iter().all(|blocked| !blocked.intersects(rect))
        && existing.iter().all(|label| !label.intersects(rect))
    {
        Some(((rect.x, rect.y), rect))
    } else {
        None
    }
}

pub(crate) fn render_diagram_canvas(
    layout: &TerminalDiagramLayout,
    selected: Option<&SymbolId>,
) -> TerminalCanvas {
    let mut canvas = TerminalCanvas::new(layout.width.max(1), layout.height.max(1));
    for edge in &layout.edges {
        for pair in edge.points.windows(2) {
            canvas.add_segment(pair[0], pair[1], edge.visual);
        }
    }
    canvas.finalize_edges();
    for edge in &layout.edges {
        if let Some((last, previous)) = edge
            .points
            .last()
            .zip(edge.points.get(edge.points.len().saturating_sub(2)))
        {
            let direction = if last.x > previous.x {
                Direction::Right
            } else if last.x < previous.x {
                Direction::Left
            } else if last.y > previous.y {
                Direction::Down
            } else {
                Direction::Up
            };
            let (x, y) = match direction {
                Direction::Right => (last.x.saturating_sub(1), last.y),
                Direction::Left => (last.x.saturating_add(1), last.y),
                Direction::Down => (last.x, last.y.saturating_sub(1)),
                Direction::Up => (last.x, last.y.saturating_add(1)),
            };
            canvas.put_arrow(x, y, direction, edge.visual);
        }
    }

    for boundary in &layout.boundaries {
        draw_box(
            &mut canvas,
            boundary.rect,
            &boundary.label,
            &[],
            if boundary.unresolved {
                TerminalDiagramStyle::Unresolved
            } else {
                TerminalDiagramStyle::Boundary
            },
            if boundary.unresolved {
                TerminalDiagramStyle::Unresolved
            } else {
                TerminalDiagramStyle::Boundary
            },
            true,
        );
    }

    for node in &layout.nodes {
        let is_selected = selected.is_some_and(|id| id == &node.id);
        let (border, text) = if is_selected {
            (
                TerminalDiagramStyle::NodeSelectedBorder,
                TerminalDiagramStyle::NodeSelectedText,
            )
        } else {
            (
                TerminalDiagramStyle::NodeBorder,
                TerminalDiagramStyle::NodeText,
            )
        };
        draw_box(
            &mut canvas,
            node.rect,
            &node.title,
            &node.lines,
            border,
            text,
            false,
        );
    }

    for edge in &layout.edges {
        if let Some((x, y)) = edge.label_position {
            canvas.put_text(x, y, &edge.label, style_for_visual(edge.visual));
        }
    }
    canvas
}

fn draw_box(
    canvas: &mut TerminalCanvas,
    rect: DiagramRect,
    title: &str,
    lines: &[String],
    border_style: TerminalDiagramStyle,
    text_style: TerminalDiagramStyle,
    dashed: bool,
) {
    canvas.fill_rect(rect, text_style);
    let horizontal = if dashed { '┄' } else { '─' };
    let vertical = if dashed { '┆' } else { '│' };
    canvas.put_char(rect.x, rect.y, '┌', border_style);
    canvas.put_char(rect.right().saturating_sub(1), rect.y, '┐', border_style);
    canvas.put_char(rect.x, rect.bottom().saturating_sub(1), '└', border_style);
    canvas.put_char(
        rect.right().saturating_sub(1),
        rect.bottom().saturating_sub(1),
        '┘',
        border_style,
    );
    for x in rect.x.saturating_add(1)..rect.right().saturating_sub(1) {
        canvas.put_char(x, rect.y, horizontal, border_style);
        canvas.put_char(x, rect.bottom().saturating_sub(1), horizontal, border_style);
    }
    for y in rect.y.saturating_add(1)..rect.bottom().saturating_sub(1) {
        canvas.put_char(rect.x, y, vertical, border_style);
        canvas.put_char(rect.right().saturating_sub(1), y, vertical, border_style);
    }
    if rect.width > 4 {
        canvas.put_text(
            rect.x.saturating_add(2),
            rect.y.saturating_add(1),
            title,
            text_style,
        );
        for (index, line) in lines.iter().enumerate() {
            if rect.y.saturating_add(2) + index as u16 >= rect.bottom().saturating_sub(1) {
                break;
            }
            canvas.put_text(
                rect.x.saturating_add(2),
                rect.y.saturating_add(2) + index as u16,
                line,
                text_style,
            );
        }
    }
}

fn truncate_text(text: &str, max_width: u16) -> String {
    if UnicodeWidthStr::width(text) as u16 <= max_width {
        return text.to_owned();
    }
    let max_width = max_width.saturating_sub(3) as f64;
    let mut output = String::new();
    let mut width = 0.0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0) as f64;
        if width + character_width > max_width {
            break;
        }
        width += character_width;
        output.push(character);
    }
    output.push_str("...");
    output
}
#[cfg(test)]
mod tests {
    use codeuml_core::{
        Provenance, RelationshipEvidence, RelationshipKind, ResolutionStatus, SourceLocation,
        SymbolId, TypeKind, Visibility,
    };
    use codeuml_diagram::{
        DiagramEdge, DiagramEndpoint, DiagramMember, DiagramMemberKind, DiagramNode,
        DiagramProjection, DiagramScope,
    };

    use super::*;

    fn location() -> SourceLocation {
        SourceLocation::new("src/lib.rs", 1, 1, 1, 10)
    }

    fn node(
        id: &str,
        name: &str,
        kind: TypeKind,
        members: &[(&str, DiagramMemberKind)],
    ) -> DiagramNode {
        DiagramNode {
            id: SymbolId::declaration("src/lib.rs", id),
            name: name.to_owned(),
            qualified_name: format!("fixture::{id}"),
            kind,
            module: None,
            location: location(),
            visibility: Visibility::Public,
            alias_of: None,
            field_count: members.len(),
            method_count: 0,
            variant_count: 0,
            members: members
                .iter()
                .map(|(text, kind)| DiagramMember {
                    kind: *kind,
                    text: (*text).to_owned(),
                })
                .collect(),
        }
    }

    fn edge(source: &DiagramNode, target: &DiagramNode, kind: RelationshipKind) -> DiagramEdge {
        DiagramEdge {
            source: DiagramEndpoint::Node(source.id.clone()),
            target: DiagramEndpoint::Node(target.id.clone()),
            kind,
            resolution: ResolutionStatus::Inferred,
            provenance: Provenance::Syntax,
            source_members: Vec::new(),
            evidence: vec![RelationshipEvidence::new(location())],
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

    fn overlapping(left: DiagramRect, right: DiagramRect) -> bool {
        left.x < right.x + right.width
            && right.x < left.x + left.width
            && left.y < right.y + right.height
            && right.y < left.y + left.height
    }

    #[test]
    fn empty_and_single_node_layouts_are_safe() {
        let empty = layout_terminal_diagram(
            &projection(Vec::new(), Vec::new()),
            &DiagramLayoutOptions::default(),
        );
        assert!(empty.nodes.is_empty());
        assert!(empty.edges.is_empty());
        assert!(empty.width > 0);
        assert!(empty.height > 0);
        let canvas = render_diagram_canvas(&empty, None);
        assert!(!canvas.to_text().contains("Alpha"));

        let alpha = node("Alpha", "Alpha", TypeKind::Struct, &[]);
        let layout = layout_terminal_diagram(
            &projection(vec![alpha.clone()], Vec::new()),
            &DiagramLayoutOptions::default(),
        );
        assert_eq!(layout.nodes.len(), 1);
        assert!(layout.node_rect(&alpha.id).is_some());
        let canvas = render_diagram_canvas(&layout, Some(&alpha.id));
        let text = canvas.to_text();
        println!("LAYOUT {:?}\n{text}", layout);
        assert!(text.contains("Alpha"));
        assert!(text.contains("struct"));
        assert!(text.contains('┌'));
        assert!(text.contains('┘'));
    }

    #[test]
    fn connected_nodes_do_not_overlap_and_edges_have_direction_and_labels() {
        let alpha = node(
            "Alpha",
            "Alpha",
            TypeKind::Struct,
            &[("id: u64", DiagramMemberKind::Field)],
        );
        let beta = node("Beta", "Beta", TypeKind::Enum, &[]);
        let layout = layout_terminal_diagram(
            &projection(
                vec![alpha.clone(), beta.clone()],
                vec![edge(&alpha, &beta, RelationshipKind::FieldType)],
            ),
            &DiagramLayoutOptions::default(),
        );
        assert_eq!(layout.nodes.len(), 2);
        assert_eq!(layout.edges.len(), 1);
        let alpha_rect = layout.node_rect(&alpha.id).unwrap();
        let beta_rect = layout.node_rect(&beta.id).unwrap();
        assert!(!overlapping(alpha_rect, beta_rect));
        assert!(layout.width >= alpha_rect.x + alpha_rect.width);
        assert!(layout.height >= beta_rect.y + beta_rect.height);
        assert_eq!(
            layout.edges[0].points.first(),
            Some(&layout.edges[0].points[0])
        );

        let canvas = render_diagram_canvas(&layout, Some(&alpha.id));
        let text = canvas.to_text();
        assert!(text.contains("Alpha"));
        assert!(text.contains("Beta"));
        assert!(text.contains("field type") || text.contains("field-type"));
        assert!(
            text.contains('>') || text.contains('v') || text.contains('▶') || text.contains('▼')
        );
    }

    #[test]
    fn multiple_relationship_kinds_self_edges_and_dense_fanout_are_handled() {
        let center = node("Center", "Center", TypeKind::Struct, &[]);
        let left = node("Left", "Left", TypeKind::Struct, &[]);
        let right = node("Right", "Right", TypeKind::Struct, &[]);
        let mut self_edge = edge(&center, &center, RelationshipKind::References);
        self_edge.kind = RelationshipKind::FieldType;
        let edges = vec![
            edge(&center, &left, RelationshipKind::FieldType),
            edge(&center, &left, RelationshipKind::MethodReturnType),
            edge(&right, &center, RelationshipKind::Implements),
            self_edge,
        ];
        let layout = layout_terminal_diagram(
            &projection(vec![center.clone(), left, right], edges),
            &DiagramLayoutOptions::default(),
        );
        assert_eq!(layout.edges.len(), 4);
        assert!(layout.edges.iter().any(|edge| edge.source == edge.target));
        let canvas = render_diagram_canvas(&layout, Some(&center.id));
        assert!(canvas.to_text().contains("implements"));
    }

    #[test]
    fn boundary_unresolved_long_unicode_and_omitted_members_are_explicit() {
        let mut alpha = node(
            "Alpha",
            "CaféWithAVeryLongTypeNameThatNeedsTruncation",
            TypeKind::Struct,
            &[
                (
                    "very_long_field_name_with_unicode_δ: SomeType",
                    DiagramMemberKind::Field,
                ),
                ("second: u32", DiagramMemberKind::Field),
                ("third: u32", DiagramMemberKind::Field),
                ("fourth: u32", DiagramMemberKind::Field),
                ("fifth: u32", DiagramMemberKind::Field),
            ],
        );
        alpha.qualified_name =
            "crate::deeply::nested::CaféWithAVeryLongTypeNameThatNeedsTruncation".to_owned();
        let beta = node("Beta", "Beta", TypeKind::Struct, &[]);
        let mut unresolved = edge(&alpha, &beta, RelationshipKind::FieldType);
        unresolved.target = DiagramEndpoint::Boundary {
            symbol: None,
            name: "Missing<δ>".to_owned(),
        };
        unresolved.resolution = ResolutionStatus::Unresolved;
        let mut boundary = edge(&alpha, &beta, RelationshipKind::TraitBound);
        boundary.target = DiagramEndpoint::Boundary {
            symbol: Some(beta.id.clone()),
            name: "api::Beta".to_owned(),
        };
        let options = DiagramLayoutOptions {
            max_members: 2,
            max_node_width: 30,
            ..DiagramLayoutOptions::default()
        };
        let layout = layout_terminal_diagram(
            &projection(vec![alpha.clone(), beta], vec![unresolved, boundary]),
            &options,
        );
        assert_eq!(layout.boundaries.len(), 2);
        assert!(layout.boundaries.iter().any(|boundary| boundary.unresolved));
        let alpha_layout = &layout.nodes[0];
        assert!(alpha_layout.rect.width <= 30);
        assert!(alpha_layout
            .lines
            .iter()
            .any(|line| line.contains("+3 more")));
        let canvas = render_diagram_canvas(&layout, Some(&alpha.id));
        let text = canvas.to_text();
        assert!(text.contains("unresolved"));
        assert!(text.contains("boundary"));
        assert!(text.contains("Café") || text.contains("Caf"));
        assert!(text.contains("+3 more"));
    }

    #[test]
    fn selection_styles_are_applied_to_the_focused_node() {
        let alpha = node("Alpha", "Alpha", TypeKind::Struct, &[]);
        let beta = node("Beta", "Beta", TypeKind::Struct, &[]);
        let layout = layout_terminal_diagram(
            &projection(vec![alpha.clone(), beta.clone()], Vec::new()),
            &DiagramLayoutOptions::default(),
        );
        let canvas = render_diagram_canvas(&layout, Some(&beta.id));
        let alpha_rect = layout.node_rect(&alpha.id).unwrap();
        let beta_rect = layout.node_rect(&beta.id).unwrap();
        let alpha_style = canvas.cell(alpha_rect.x, alpha_rect.y).unwrap().style;
        let beta_style = canvas.cell(beta_rect.x, beta_rect.y).unwrap().style;
        assert_ne!(alpha_style, beta_style);
        assert!(matches!(
            beta_style,
            TerminalDiagramStyle::NodeSelectedBorder
        ));
    }

    #[test]
    fn layout_and_canvas_are_deterministic() {
        let a = node("A", "Alpha", TypeKind::Struct, &[]);
        let b = node("B", "Beta", TypeKind::Struct, &[]);
        let projection = projection(
            vec![a.clone(), b.clone()],
            vec![edge(&a, &b, RelationshipKind::FieldType)],
        );
        let options = DiagramLayoutOptions::default();
        let first = layout_terminal_diagram(&projection, &options);
        let second = layout_terminal_diagram(&projection, &options);
        assert_eq!(first, second);
        assert_eq!(
            render_diagram_canvas(&first, Some(&a.id)),
            render_diagram_canvas(&second, Some(&a.id))
        );
    }

    #[test]
    fn large_dense_graphs_do_not_overlap_or_recurse_unboundedly() {
        let nodes: Vec<_> = (0..120)
            .map(|index| {
                node(
                    &format!("N{index}"),
                    &format!("Node{index}"),
                    TypeKind::Struct,
                    &[],
                )
            })
            .collect();
        let mut edges = Vec::new();
        for pair in nodes.windows(2) {
            edges.push(edge(&pair[0], &pair[1], RelationshipKind::FieldType));
        }
        for index in (0..110).step_by(7) {
            edges.push(edge(
                &nodes[index],
                &nodes[index + 10],
                RelationshipKind::References,
            ));
        }
        let layout = layout_terminal_diagram(
            &projection(nodes, edges),
            &DiagramLayoutOptions {
                max_canvas_width: 120,
                ..DiagramLayoutOptions::default()
            },
        );
        assert_eq!(layout.nodes.len(), 120);
        for (index, left) in layout.nodes.iter().enumerate() {
            for right in layout.nodes.iter().skip(index + 1) {
                assert!(!overlapping(left.rect, right.rect));
            }
        }
        let canvas = render_diagram_canvas(&layout, None);
        assert!(canvas.width > 0 && canvas.height > 0);
    }
}
