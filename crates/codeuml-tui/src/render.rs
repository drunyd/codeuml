use codeuml_metrics::CrapLevel;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, ArchitecturePreview, ArchitectureRow, Focus, Mode},
    format::{self, DetailRow},
    score::{fit_row, CrapScore},
    terminal_diagram::TerminalDiagramStyle,
};

/// Columns a list row can use: the block borders and the highlight symbol take 4.
fn list_content_width(area: Rect) -> usize {
    usize::from(area.width.saturating_sub(4))
}

fn crap_color(level: CrapLevel) -> Color {
    match level {
        CrapLevel::Good => Color::Green,
        CrapLevel::Medium => Color::Yellow,
        CrapLevel::Bad => Color::Red,
    }
}

/// A row with the label on the left and the score right-aligned and colour-coded.
fn score_line(
    label: &str,
    label_style: Style,
    score: Option<CrapScore>,
    width: usize,
) -> Line<'static> {
    let score_text = score.map(|score| score.text());
    let (label, padding) = fit_row(label, score_text.as_deref(), width);
    let mut spans = vec![Span::styled(label, label_style)];
    if let (Some(score), Some(text), Some(padding)) = (score, score_text, padding) {
        spans.push(Span::raw(padding));
        spans.push(Span::styled(
            text,
            Style::default()
                .fg(crap_color(score.level))
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

pub(crate) fn render(frame: &mut Frame<'_>, app: &mut App<'_>) {
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(frame.area());

    render_header(frame, app, chunks[0]);
    let background = background_mode(app);
    render_main_view(frame, app, chunks[1], background);
    render_footer(frame, app, chunks[2]);

    match app.mode() {
        Mode::Help => render_help(frame, chunks[1]),
        Mode::ScopeDialog => render_scope_dialog(frame, app, chunks[1]),
        _ => {}
    }
}

fn background_mode(app: &App<'_>) -> Mode {
    match app.mode() {
        Mode::Architecture => Mode::Architecture,
        Mode::Diagram => Mode::Diagram,
        Mode::Help | Mode::ScopeDialog => app.overlay_return(),
        _ => Mode::Browse,
    }
}

fn render_main_view(frame: &mut Frame<'_>, app: &mut App<'_>, area: Rect, background: Mode) {
    if app.mode() == Mode::Diagnostics {
        render_diagnostics(frame, app, area);
    } else if app.mode() == Mode::Source {
        render_source(frame, app, area);
    } else if background == Mode::Diagram {
        render_diagram(frame, app, area);
    } else if background == Mode::Architecture {
        render_architecture(frame, app, area);
    } else {
        render_browser(frame, app, area);
    }
}

fn render_diagram(frame: &mut Frame<'_>, app: &mut App<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Diagram - {}", app.current_scope_label()));
    let inner = block.inner(area);
    app.set_diagram_viewport(inner.width, inner.height);
    frame.render_widget(block, area);

    if app.diagram_layout().nodes.is_empty() {
        frame.render_widget(
            Paragraph::new("No types in the selected scope.")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let canvas = app.diagram_canvas();
    let (pan_x, pan_y) = app.diagram_pan();
    let lines = diagram_lines(canvas, inner, pan_x, pan_y);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn diagram_lines(
    canvas: &crate::terminal_diagram::TerminalCanvas,
    viewport: Rect,
    pan_x: u16,
    pan_y: u16,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for row in 0..viewport.height {
        let canvas_y = pan_y.saturating_add(row);
        if canvas_y >= canvas.height {
            break;
        }
        lines.push(diagram_row(canvas, viewport.width, pan_x, canvas_y));
    }
    lines
}

fn diagram_row(
    canvas: &crate::terminal_diagram::TerminalCanvas,
    width: u16,
    pan_x: u16,
    canvas_y: u16,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current_style = Style::default();
    let mut current_text = String::new();
    for column in 0..width {
        let canvas_x = pan_x.saturating_add(column);
        if canvas_x >= canvas.width {
            break;
        }
        let Some(cell) = canvas.cell(canvas_x, canvas_y) else {
            continue;
        };
        if cell.continuation {
            add_continuation_span(
                &mut spans,
                &mut current_text,
                current_style,
                canvas_x == pan_x,
            );
            continue;
        }
        let style = diagram_cell_style(cell.style);
        if !current_text.is_empty() && style != current_style {
            spans.push(Span::styled(
                std::mem::take(&mut current_text),
                current_style,
            ));
        }
        current_style = style;
        current_text.push(cell.ch);
    }
    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }
    Line::from(spans)
}

fn add_continuation_span(
    spans: &mut Vec<Span<'static>>,
    current_text: &mut String,
    current_style: Style,
    clipped_at_start: bool,
) {
    if !clipped_at_start {
        return;
    }
    if current_style != Style::default() {
        spans.push(Span::styled(std::mem::take(current_text), current_style));
    } else {
        current_text.clear();
    }
    spans.push(Span::raw(" "));
}

fn render_source(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let Some(view) = app.source_view() else {
        return;
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Source - {} ({})", view.title, view.file));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let number_width = view.lines.len().max(1).to_string().len();
    let lines: Vec<Line<'static>> = view
        .lines
        .iter()
        .enumerate()
        .skip(app.source_scroll())
        .take(usize::from(inner.height))
        .map(|(index, text)| {
            let highlighted = (view.highlight.0..=view.highlight.1).contains(&index);
            let style = if highlighted {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            Line::from(vec![
                Span::styled(
                    format!("{:>number_width$} ", index + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(text.clone(), style),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_architecture(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let has_parent_pane = area.width >= 90;
    let chunks = if has_parent_pane {
        Layout::horizontal([
            Constraint::Percentage(22),
            Constraint::Percentage(33),
            Constraint::Percentage(45),
        ])
        .split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).split(area)
    };
    let (current_area, preview_area) = if has_parent_pane {
        match app.architecture_parent_pane() {
            Some((title, rows, selected)) => {
                render_architecture_list(frame, chunks[0], &title, &rows, Some(selected), false);
            }
            None => frame.render_widget(
                Paragraph::new("Project root")
                    .style(Style::default().fg(Color::DarkGray))
                    .block(Block::default().borders(Borders::ALL).title("Parent")),
                chunks[0],
            ),
        }
        (chunks[1], chunks[2])
    } else {
        (chunks[0], chunks[1])
    };

    let rows = app.architecture_rows();
    let selected = (!rows.is_empty()).then(|| app.architecture_selected());
    render_architecture_list(
        frame,
        current_area,
        &app.architecture_path_label(),
        &rows,
        selected,
        true,
    );

    match app.architecture_preview() {
        ArchitecturePreview::Children { title, rows } => {
            if rows.is_empty() {
                frame.render_widget(
                    Paragraph::new("Empty module.")
                        .style(Style::default().fg(Color::DarkGray))
                        .block(Block::default().borders(Borders::ALL).title(title)),
                    preview_area,
                );
            } else {
                render_architecture_list(frame, preview_area, &title, &rows, None, false);
            }
        }
        ArchitecturePreview::Details { title, lines } => frame.render_widget(
            Paragraph::new(detail_lines(
                &lines,
                usize::from(preview_area.width.saturating_sub(2)),
            ))
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false }),
            preview_area,
        ),
        ArchitecturePreview::Empty => frame.render_widget(
            Paragraph::new("Nothing selected.")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL).title("Preview")),
            preview_area,
        ),
    }
}

fn detail_lines(rows: &[DetailRow], width: usize) -> Vec<Line<'static>> {
    rows.iter()
        .map(|row| match row.crap {
            Some(score) => score_line(&row.text, Style::default(), Some(score), width),
            None => Line::from(row.text.clone()),
        })
        .collect()
}

/// The selected row of an active list keeps white text on the blue highlight;
/// the highlight itself sets no foreground so score colours survive on it.
fn architecture_label_style(is_module: bool, selected: bool) -> Style {
    match (selected, is_module) {
        (true, _) => Style::default().fg(Color::White),
        (false, true) => Style::default().fg(Color::Cyan),
        (false, false) => Style::default(),
    }
}

fn render_architecture_list(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    rows: &[ArchitectureRow],
    selected: Option<usize>,
    active: bool,
) {
    let width = list_content_width(area);
    let items: Vec<ListItem<'_>> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let style = architecture_label_style(row.is_module, active && selected == Some(index));
            ListItem::new(score_line(&row.label, style, row.crap, width))
        })
        .collect();
    let mut state = ListState::default();
    state.select(selected);
    let highlight = if active {
        Style::default()
            .bg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    };
    let border = if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border)
                .title(title.to_owned()),
        )
        .highlight_style(highlight)
        .highlight_symbol(if active { "> " } else { "  " });
    frame.render_stateful_widget(list, area, &mut state);
}

fn diagram_cell_style(style: TerminalDiagramStyle) -> Style {
    match style {
        TerminalDiagramStyle::Blank => Style::default(),
        TerminalDiagramStyle::NodeBorder
        | TerminalDiagramStyle::NodeText
        | TerminalDiagramStyle::NodeSelectedBorder
        | TerminalDiagramStyle::NodeSelectedText => node_cell_style(style),
        TerminalDiagramStyle::Boundary | TerminalDiagramStyle::Unresolved => {
            boundary_cell_style(style)
        }
        TerminalDiagramStyle::EdgeDefault
        | TerminalDiagramStyle::EdgeMethod
        | TerminalDiagramStyle::EdgeImplements
        | TerminalDiagramStyle::EdgeTrait
        | TerminalDiagramStyle::EdgeAlias
        | TerminalDiagramStyle::EdgeBoundary
        | TerminalDiagramStyle::EdgeUnresolved => edge_cell_style(style),
    }
}

fn node_cell_style(style: TerminalDiagramStyle) -> Style {
    match style {
        TerminalDiagramStyle::NodeBorder => Style::default().fg(Color::Gray),
        TerminalDiagramStyle::NodeText => Style::default().fg(Color::White),
        TerminalDiagramStyle::NodeSelectedBorder | TerminalDiagramStyle::NodeSelectedText => {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        }
        _ => Style::default(),
    }
}

fn boundary_cell_style(style: TerminalDiagramStyle) -> Style {
    match style {
        TerminalDiagramStyle::Boundary => Style::default().fg(Color::Yellow),
        TerminalDiagramStyle::Unresolved => Style::default().fg(Color::Red),
        _ => Style::default(),
    }
}

fn edge_cell_style(style: TerminalDiagramStyle) -> Style {
    match style {
        TerminalDiagramStyle::EdgeDefault => Style::default().fg(Color::Gray),
        TerminalDiagramStyle::EdgeMethod => Style::default().fg(Color::DarkGray),
        TerminalDiagramStyle::EdgeImplements => Style::default().fg(Color::Blue),
        TerminalDiagramStyle::EdgeTrait => Style::default().fg(Color::Magenta),
        TerminalDiagramStyle::EdgeAlias => Style::default().fg(Color::Cyan),
        TerminalDiagramStyle::EdgeBoundary => Style::default().fg(Color::Yellow),
        TerminalDiagramStyle::EdgeUnresolved => Style::default().fg(Color::Red),
        _ => Style::default(),
    }
}

fn render_header(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let projection = app.projection();
    let title = Line::from(vec![
        Span::styled(
            app.project_name().to_owned(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  |  "),
        Span::styled(
            app.current_scope_label(),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(format!(
            "  |  {} types  |  {} edges  |  {} diagnostics{}",
            projection.nodes.len(),
            projection.edges.len(),
            app.diagnostics().len(),
            if app.history_depth() > 0 {
                format!("  |  Backspace: back ({})", app.history_depth())
            } else {
                String::new()
            }
        )),
    ]);
    let mut status = vec![Span::styled(
        app.analysis_label(),
        Style::default().fg(Color::DarkGray),
    )];
    if let Some(legend) = app.crap_legend() {
        status.push(Span::styled(
            format!("  |  {legend}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    let header = Paragraph::new(vec![title, Line::from(status)])
        .block(Block::default().borders(Borders::ALL).title("CodeUML"));
    frame.render_widget(header, area);
}

fn render_browser(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let chunks = if area.width >= 100 {
        Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Percentage(45),
            Constraint::Percentage(25),
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Percentage(35),
            Constraint::Percentage(40),
            Constraint::Percentage(25),
        ])
        .split(area)
    };

    render_type_list(frame, app, chunks[0]);
    render_details(frame, app, chunks[1]);
    render_relationships(frame, app, chunks[2]);
}

fn render_type_list(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(format!(
        "Types ({}/{})",
        if app.visible_type_count() == 0 {
            0
        } else {
            app.selected_visible_index().unwrap_or(0) + 1
        },
        app.visible_type_count()
    ));
    let nodes = app.visible_nodes();
    if nodes.is_empty() {
        let message = if app.search_query().is_empty() {
            "No types in scope."
        } else {
            "No matching types."
        };
        frame.render_widget(
            Paragraph::new(message)
                .block(block)
                .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let width = list_content_width(area);
    let selected = app.selected_visible_index();
    let items: Vec<ListItem<'_>> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let label = if node.qualified_name == node.name {
                format!("{}  [{}]", node.name, node.kind)
            } else {
                format!("{}  [{}]  {}", node.name, node.kind, node.qualified_name)
            };
            let style = if selected == Some(index) {
                Style::default().fg(Color::White)
            } else {
                Style::default()
            };
            ListItem::new(score_line(
                &label,
                style,
                app.type_crap(&node.qualified_name),
                width,
            ))
        })
        .collect();
    let mut state = ListState::default();
    state.select(selected);
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_details(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let title = app
        .selected_node()
        .map(|node| format!("Details: {}", node.name))
        .unwrap_or_else(|| "Details".to_owned());
    let lines = detail_lines(
        &app.detail_rows(),
        usize::from(area.width.saturating_sub(2)),
    );
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .scroll((app.detail_scroll(), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_relationships(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let rows = app.relationship_rows();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Relationships ({})", rows.len()));
    if rows.is_empty() {
        frame.render_widget(Paragraph::new("No relationships.").block(block), area);
        return;
    }

    let items: Vec<ListItem<'_>> = rows
        .iter()
        .map(|row| {
            let style = match (&row.endpoint, row.direction, row.resolution) {
                (_, _, codeuml_core::ResolutionStatus::Unresolved) => {
                    Style::default().fg(Color::Red)
                }
                (codeuml_diagram::DiagramEndpoint::Boundary { .. }, _, _) => {
                    Style::default().fg(Color::Yellow)
                }
                _ => Style::default(),
            };
            ListItem::new(Line::from(Span::styled(
                app.relationship_row_text(row),
                style,
            )))
        })
        .collect();
    let mut state = ListState::default();
    if app.focus() == Focus::Relationships {
        state.select(Some(app.relationship_selected()));
    }
    let style = if app.focus() == Focus::Relationships {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(block.border_style(style))
        .highlight_style(
            Style::default()
                .bg(Color::Magenta)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_diagnostics(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Diagnostics ({})", app.diagnostics().len()));
    if app.diagnostics().is_empty() {
        frame.render_widget(Paragraph::new("No diagnostics.").block(block), area);
        return;
    }

    let items: Vec<ListItem<'_>> = app
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            let style = match diagnostic.severity {
                codeuml_core::DiagnosticSeverity::Error => Style::default().fg(Color::Red),
                codeuml_core::DiagnosticSeverity::Warning => Style::default().fg(Color::Yellow),
                codeuml_core::DiagnosticSeverity::Info => Style::default().fg(Color::Cyan),
                _ => Style::default(),
            };
            ListItem::new(Line::from(Span::styled(
                format::diagnostic_line(diagnostic),
                style,
            )))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.diagnostic_selected()));
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_footer(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let status = if app.mode() == Mode::Search {
        format!("Search: {}_", app.search_query())
    } else {
        app.status().to_owned()
    };
    let keys = match app.mode() {
        Mode::Browse if app.focus() == Focus::Relationships => {
            "Up/Down select  Enter follow  o evidence source  Backspace back  Tab types  s scope  d diagnostics  ? help  q quit"
        }
        Mode::Browse => {
            "Up/Down or j/k move  Tab panels  / search  o source  a architecture  s scope  d diagnostics  ? help  q quit"
        }
        Mode::Source => "Up/Down PgUp/PgDn Home/End scroll  Esc/b/o close  Ctrl+C quit",
        Mode::Architecture => {
            "j/k move  l/Enter open  h up a level  o source  Esc/b up or exit  s scope  d diagnostics  ? help  q quit"
        }
        Mode::Search => "Type to filter  Backspace  Enter accept  Esc cancel  Ctrl+C quit",
        Mode::Diagnostics => "Up/Down scroll  PgUp/PgDn  Home/End  Esc back  Ctrl+C quit",
        Mode::ScopeDialog => "Up/Down select  Enter apply  Esc cancel",
        Mode::Help => "Esc or ? close help",
        Mode::Diagram => {
            "Arrows/hjkl pan  Tab/BackTab or n/p focus  c center  o source  Enter details  Esc/b browser  s scope  d diagnostics  ? help  q quit"
        }
    };
    let footer = Paragraph::new(vec![
        Line::from(Span::styled(status, Style::default().fg(Color::Yellow))),
        Line::from(Span::styled(keys, Style::default().fg(Color::DarkGray))),
    ])
    .wrap(Wrap { trim: true });
    frame.render_widget(footer, area);
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(area, 76, 82);
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from(Span::styled(
            "Keyboard shortcuts",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Up/Down, j/k       Move in the focused list"),
        Line::from("PageUp/PageDown    Move by a page"),
        Line::from("Home/End           First or last item"),
        Line::from("Tab                Switch type list / relationships"),
        Line::from("Enter              Follow selected relationship"),
        Line::from("/                  Incremental search (name or qualified name)"),
        Line::from("Esc                Clear search / close view"),
        Line::from("s                  Choose project, module, or neighborhood scope"),
        Line::from("a                  Browse the hierarchical module architecture"),
        Line::from("d                  Diagnostics browser"),
        Line::from("v                  Open the terminal UML diagram"),
        Line::from("o                  Show source for the selected type, module, or evidence"),
        Line::from("Backspace          Go back after following a relationship"),
        Line::from("Ctrl+U / Ctrl+D    Scroll type details"),
        Line::from("?                  Toggle this help"),
        Line::from("q                  Quit from the browser"),
        Line::from("Ctrl+C             Quit from any view"),
        Line::from(""),
        Line::from("Diagram: arrows/hjkl pan; Tab/BackTab or n/p focus; c center."),
        Line::from("Diagram: Enter opens details; Esc/b/v returns to the browser."),
        Line::from("Architecture: j/k move, l/Enter open module/type, h up a level, Esc/b exit."),
        Line::from("Resolved relationships navigate to their target type."),
        Line::from(
            "Boundary targets may open in project scope; unresolved targets do not navigate.",
        ),
    ];
    let help = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Help - Keyboard shortcuts"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(help, popup);
}

fn render_scope_dialog(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let popup = centered_rect(area, 72, 68);
    frame.render_widget(Clear, popup);
    let labels = app.scope_choice_labels();
    let items: Vec<ListItem<'_>> = labels
        .iter()
        .map(|label| ListItem::new(Line::from((*label).to_owned())))
        .collect();
    let mut state = ListState::default();
    if !labels.is_empty() {
        state.select(Some(app.scope_choice_selected()));
    }
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Select scope (Enter apply, Esc cancel)"),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, popup, &mut state);
}

fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let width = area.width.saturating_mul(percent_x) / 100;
    let height = area.height.saturating_mul(percent_y) / 100;
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use codeuml_core::Codebase;
    use ratatui::{backend::TestBackend, Terminal};

    use super::*;
    use crate::app::Action;
    use crate::terminal_diagram::TerminalCanvas;
    use crate::test_support::{fixture_codebase, fixture_metrics};

    #[test]
    fn score_lines_are_right_aligned_and_colour_coded_by_band() {
        let line = |level, value, width| {
            score_line(
                "Alpha",
                Style::default(),
                Some(CrapScore::new(level, value)),
                width,
            )
        };
        for (level, color) in [
            (CrapLevel::Good, Color::Green),
            (CrapLevel::Medium, Color::Yellow),
            (CrapLevel::Bad, Color::Red),
        ] {
            let line = line(level, 14.5, 20);
            assert_eq!(line.width(), 20);
            let score = line.spans.last().expect("score span");
            assert_eq!(score.content, "14.5");
            assert_eq!(score.style.fg, Some(color));
        }

        let narrow = line(CrapLevel::Bad, 14.5, 5);
        assert_eq!(narrow.spans.len(), 1);
        assert_eq!(narrow.width(), 5);
        let unscored = score_line("Alpha", Style::default(), None, 20);
        assert_eq!(unscored.spans.len(), 1);
        assert_eq!(unscored.width(), 5);
    }

    #[test]
    fn type_list_architecture_and_details_show_scores_and_the_legend() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase)
            .expect("app")
            .with_metrics(Some(fixture_metrics()));
        let text = render_text(&mut app, 120, 36);
        assert!(
            text.contains("CRAP: green <= 6, yellow <= 12, red above"),
            "{text}"
        );
        let alpha_row: String = text
            .lines()
            .find(|line| line.contains("Alpha  [struct]"))
            .expect("alpha row")
            .chars()
            .take(36)
            .collect();
        assert!(alpha_row.ends_with("14.5│"), "{alpha_row}");
        let detail_row = text
            .lines()
            .find(|line| line.contains("find(id: u64)"))
            .expect("method row");
        assert!(detail_row.contains("14.5"), "{detail_row}");
        let measure_row = text
            .lines()
            .find(|line| line.contains("measure()"))
            .expect("method row");
        assert!(measure_row.contains("3.0"), "{measure_row}");

        app.handle(Action::OpenArchitecture);
        let text = render_text(&mut app, 120, 36);
        let api_row = text
            .lines()
            .find(|line| line.contains("> api/"))
            .expect("api row");
        assert!(api_row.contains("8.0"), "{api_row}");
        assert!(text.contains("14.5"), "{text}");
    }

    #[test]
    fn selected_rows_keep_their_score_colour() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase)
            .expect("app")
            .with_metrics(Some(fixture_metrics()));
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).expect("terminal");
        terminal
            .draw(|frame| render(frame, &mut app))
            .expect("draw");
        let buffer = terminal.backend().buffer();

        let selected_score = (0..buffer.area.height)
            .flat_map(|y| (0..buffer.area.width - 3).map(move |x| (x, y)))
            .find(|&(x, y)| {
                let text: String = (0..4).map(|i| buffer[(x + i, y)].symbol()).collect();
                text == "14.5" && buffer[(x, y)].bg == Color::Blue
            })
            .expect("score on the highlighted list row");
        assert_eq!(buffer[selected_score].fg, Color::Red);
    }

    fn render_text(app: &mut App<'_>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render(frame, app))
            .expect("render frame");
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn main_layout_shows_header_types_details_relationships_and_help_keys() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        let text = render_text(&mut app, 120, 36);

        assert!(text.contains("fixture-project"), "{text}");
        assert!(text.contains("Project"), "{text}");
        assert!(text.contains("Types"), "{text}");
        assert!(text.contains("Alpha"), "{text}");
        assert!(text.contains("models::Alpha"), "{text}");
        assert!(text.contains("Details"), "{text}");
        assert!(text.contains("Fields"), "{text}");
        assert!(text.contains("Relationships"), "{text}");
        assert!(text.contains("unresolved"), "{text}");
        assert!(text.contains("/ search"), "{text}");
        assert!(text.contains("? help"), "{text}");
    }

    #[test]
    fn small_terminals_render_without_panicking() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        for (width, height) in [(20, 5), (10, 3), (1, 1)] {
            let text = render_text(&mut app, width, height);
            assert_eq!(
                text.lines().count(),
                usize::from(height),
                "render size {width}x{height}"
            );
        }
    }

    #[test]
    fn empty_project_and_empty_search_have_explicit_messages() {
        let empty = Codebase::new("empty");
        let mut empty_app = App::new(&empty).expect("empty app");
        let text = render_text(&mut empty_app, 80, 24);
        assert!(text.contains("No types"), "{text}");
        assert!(text.contains("No type selected"), "{text}");

        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        app.handle(Action::StartSearch);
        for character in "zzzz".chars() {
            app.handle(Action::SearchInput(character));
        }
        let text = render_text(&mut app, 90, 24);
        assert!(text.contains("No matching types"), "{text}");
        assert!(text.contains("Search: zzzz"), "{text}");
    }

    #[test]
    fn help_and_scope_dialogs_are_visible_and_discoverable() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        app.handle(Action::OpenHelp);
        let text = render_text(&mut app, 120, 36);
        assert!(text.contains("Keyboard shortcuts"), "{text}");
        assert!(text.contains("Ctrl+C"), "{text}");
        assert!(text.contains("terminal UML diagram"), "{text}");
        assert!(text.contains("Diagram: arrows"), "{text}");

        app.handle(Action::Cancel);
        app.handle(Action::OpenScope);
        let text = render_text(&mut app, 100, 30);
        assert!(text.contains("Select scope"), "{text}");
        assert!(text.contains("Project"), "{text}");
        assert!(text.contains("Module: models"), "{text}");
        assert!(text.contains("Neighborhood: models::Alpha"), "{text}");
    }

    #[test]
    fn diagnostics_view_displays_severity_location_and_empty_state() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        app.handle(Action::OpenDiagnostics);
        let text = render_text(&mut app, 120, 30);
        assert!(text.contains("Diagnostics"), "{text}");
        assert!(text.contains("[warning]"), "{text}");
        assert!(text.contains("src/models.rs:5:1"), "{text}");
        assert!(text.contains("fixture file failed to parse"), "{text}");

        let empty = Codebase::new("empty");
        let mut empty_app = App::new(&empty).expect("empty app");
        empty_app.handle(Action::OpenDiagnostics);
        let text = render_text(&mut empty_app, 80, 24);
        assert!(text.contains("No diagnostics"), "{text}");
    }

    #[test]
    fn diagram_view_renders_nodes_edges_and_controls_and_clips_small_viewports() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        app.handle(Action::OpenDiagram);
        let text = render_text(&mut app, 120, 36);
        assert!(text.contains("Diagram - Project"), "{text}");
        assert!(text.contains("Alpha"), "{text}");
        assert!(text.contains("Beta"), "{text}");
        assert!(
            text.contains("field type") || text.contains("field-type"),
            "{text}"
        );
        assert!(text.contains("pan"), "{text}");
        assert!(text.contains("Enter details"), "{text}");

        app.set_diagram_viewport(20, 6);
        app.handle(Action::PanRight);
        assert!(app.diagram_pan().0 > 0);
        let text = render_text(&mut app, 20, 6);
        assert_eq!(text.lines().count(), 6);
    }

    #[test]
    fn architecture_view_shows_parent_current_and_preview_panes() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        app.handle(Action::OpenArchitecture);
        let text = render_text(&mut app, 120, 32);

        assert!(text.contains("Project root"), "{text}");
        assert!(text.contains("fixture-project"), "{text}");
        assert!(text.contains("> api/"), "{text}");
        assert!(text.contains("models/"), "{text}");
        assert!(text.contains("api (src/api.rs:1)"), "{text}");
        assert!(text.contains("Api  [struct]"), "{text}");
        assert!(text.contains("h up a level"), "{text}");

        app.handle(Action::NavigateDown);
        app.handle(Action::Descend);
        let text = render_text(&mut app, 120, 32);
        assert!(text.contains("Alpha  [struct]"), "{text}");
        assert!(text.contains("Beta  [struct]"), "{text}");
        assert!(text.contains("Shape  [enum]"), "{text}");
        assert!(text.contains("Name: Alpha"), "{text}");
        assert!(text.contains("Fields"), "{text}");
        assert!(!text.contains("Project root"), "{text}");

        let narrow = render_text(&mut app, 70, 24);
        assert!(narrow.contains("Alpha  [struct]"), "{narrow}");
    }

    #[test]
    fn architecture_view_shows_modules_and_selected_module_preview() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        app.handle(Action::OpenArchitecture);
        let text = render_text(&mut app, 120, 32);

        assert!(text.contains("api/"), "{text}");
        assert!(text.contains("src/api.rs"), "{text}");
        assert!(text.contains("l/Enter open"), "{text}");
    }

    #[test]
    fn source_view_renders_numbered_lines_with_title_and_controls() {
        let project = crate::test_support::fixture_project("render-source");
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase)
            .expect("app")
            .with_project_root(project.0.clone());
        app.handle(Action::OpenSource);
        let text = render_text(&mut app, 120, 30);
        assert!(text.contains("Source - models::Alpha"), "{text}");
        assert!(text.contains("// models line 3"), "{text}");
        assert!(text.contains("Esc/b/o close"), "{text}");

        app.handle(Action::Cancel);
        app.handle(Action::ToggleFocus);
        app.handle(Action::Confirm);
        let text = render_text(&mut app, 140, 30);
        assert!(text.contains("Backspace: back (1)"), "{text}");
    }

    #[test]
    fn relationship_panel_shows_direction_boundary_and_incoming_labels() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("app");
        let text = render_text(&mut app, 140, 36);
        assert!(text.contains("OUT"), "{text}");
        assert!(text.contains("IN"), "{text}");
        assert!(
            text.contains("boundary") || text.contains("unresolved"),
            "{text}"
        );
        assert!(text.contains("api::Api"), "{text}");
    }

    #[test]
    fn diagram_styles_map_every_canvas_variant() {
        let cases = [
            (TerminalDiagramStyle::Blank, None),
            (TerminalDiagramStyle::NodeBorder, Some(Color::Gray)),
            (TerminalDiagramStyle::NodeText, Some(Color::White)),
            (
                TerminalDiagramStyle::NodeSelectedBorder,
                Some(Color::Yellow),
            ),
            (TerminalDiagramStyle::NodeSelectedText, Some(Color::Yellow)),
            (TerminalDiagramStyle::Boundary, Some(Color::Yellow)),
            (TerminalDiagramStyle::Unresolved, Some(Color::Red)),
            (TerminalDiagramStyle::EdgeDefault, Some(Color::Gray)),
            (TerminalDiagramStyle::EdgeMethod, Some(Color::DarkGray)),
            (TerminalDiagramStyle::EdgeImplements, Some(Color::Blue)),
            (TerminalDiagramStyle::EdgeTrait, Some(Color::Magenta)),
            (TerminalDiagramStyle::EdgeAlias, Some(Color::Cyan)),
            (TerminalDiagramStyle::EdgeBoundary, Some(Color::Yellow)),
            (TerminalDiagramStyle::EdgeUnresolved, Some(Color::Red)),
        ];
        for (diagram_style, foreground) in cases {
            assert_eq!(diagram_cell_style(diagram_style).fg, foreground);
        }
        let selected = diagram_cell_style(TerminalDiagramStyle::NodeSelectedBorder);
        assert_eq!(selected.add_modifier(Modifier::BOLD), selected);
    }

    #[test]
    fn panned_wide_character_continuation_and_canvas_bounds_render_safely() {
        let canvas = TerminalCanvas::with_wide_text();
        let lines = diagram_lines(&canvas, Rect::new(0, 0, 4, 4), 1, 0);
        assert_eq!(lines.len(), 1);
        let visible: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(visible, " x ");
    }
}
