use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, Focus, Mode},
    format,
    terminal_diagram::TerminalDiagramStyle,
};

pub(crate) fn render(frame: &mut Frame<'_>, app: &mut App<'_>) {
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(frame.area());

    render_header(frame, app, chunks[0]);
    let background = match app.mode() {
        Mode::Diagram => Mode::Diagram,
        Mode::Help | Mode::ScopeDialog => app.overlay_return(),
        _ => Mode::Browse,
    };
    if app.mode() == Mode::Diagnostics {
        render_diagnostics(frame, app, chunks[1]);
    } else if background == Mode::Diagram {
        render_diagram(frame, app, chunks[1]);
    } else {
        render_browser(frame, app, chunks[1]);
    }
    render_footer(frame, app, chunks[2]);

    match app.mode() {
        Mode::Help => render_help(frame, chunks[1]),
        Mode::ScopeDialog => render_scope_dialog(frame, app, chunks[1]),
        _ => {}
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
    let mut lines = Vec::new();
    for row in 0..inner.height {
        let canvas_y = pan_y.saturating_add(row);
        if canvas_y >= canvas.height {
            break;
        }
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut current_style = Style::default();
        let mut current_text = String::new();
        for column in 0..inner.width {
            let canvas_x = pan_x.saturating_add(column);
            if canvas_x >= canvas.width {
                break;
            }
            let Some(cell) = canvas.cell(canvas_x, canvas_y) else {
                continue;
            };
            if cell.continuation {
                if canvas_x == pan_x {
                    if current_style != Style::default() {
                        spans.push(Span::styled(
                            std::mem::take(&mut current_text),
                            current_style,
                        ));
                    } else {
                        current_text.clear();
                    }
                    spans.push(Span::raw(" "));
                }
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
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn diagram_cell_style(style: TerminalDiagramStyle) -> Style {
    match style {
        TerminalDiagramStyle::Blank => Style::default(),
        TerminalDiagramStyle::NodeBorder => Style::default().fg(Color::Gray),
        TerminalDiagramStyle::NodeText => Style::default().fg(Color::White),
        TerminalDiagramStyle::NodeSelectedBorder => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        TerminalDiagramStyle::NodeSelectedText => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        TerminalDiagramStyle::Boundary => Style::default().fg(Color::Yellow),
        TerminalDiagramStyle::Unresolved => Style::default().fg(Color::Red),
        TerminalDiagramStyle::EdgeDefault => Style::default().fg(Color::Gray),
        TerminalDiagramStyle::EdgeMethod => Style::default().fg(Color::DarkGray),
        TerminalDiagramStyle::EdgeImplements => Style::default().fg(Color::Blue),
        TerminalDiagramStyle::EdgeTrait => Style::default().fg(Color::Magenta),
        TerminalDiagramStyle::EdgeAlias => Style::default().fg(Color::Cyan),
        TerminalDiagramStyle::EdgeBoundary => Style::default().fg(Color::Yellow),
        TerminalDiagramStyle::EdgeUnresolved => Style::default().fg(Color::Red),
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
            "  |  {} types  |  {} edges  |  {} diagnostics",
            projection.nodes.len(),
            projection.edges.len(),
            app.diagnostics().len()
        )),
    ]);
    let header = Paragraph::new(vec![
        title,
        Line::from(Span::styled(
            app.analysis_label(),
            Style::default().fg(Color::DarkGray),
        )),
    ])
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

    let items: Vec<ListItem<'_>> = nodes
        .iter()
        .map(|node| {
            let label = if node.qualified_name == node.name {
                format!("{}  [{}]", node.name, node.kind)
            } else {
                format!("{}  [{}]  {}", node.name, node.kind, node.qualified_name)
            };
            ListItem::new(Line::from(label))
        })
        .collect();
    let mut state = ListState::default();
    state.select(app.selected_visible_index());
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

fn render_details(frame: &mut Frame<'_>, app: &App<'_>, area: Rect) {
    let title = app
        .selected_node()
        .map(|node| format!("Details: {}", node.name))
        .unwrap_or_else(|| "Details".to_owned());
    let paragraph = Paragraph::new(app.detail_lines().join("\n"))
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
            "Up/Down select  Enter follow  Tab types  s scope  d diagnostics  ? help  q quit  Ctrl+C quit"
        }
        Mode::Browse => {
            "Up/Down or j/k move  Tab panels  / search  s scope  d diagnostics  ? help  q quit"
        }
        Mode::Search => "Type to filter  Backspace  Enter accept  Esc cancel  Ctrl+C quit",
        Mode::Diagnostics => "Up/Down scroll  PgUp/PgDn  Home/End  Esc back  Ctrl+C quit",
        Mode::ScopeDialog => "Up/Down select  Enter apply  Esc cancel",
        Mode::Help => "Esc or ? close help",
        Mode::Diagram => {
            "Arrows/hjkl pan  Tab/BackTab or n/p focus  c center  Enter details  Esc/b browser  s scope  d diagnostics  ? help  q quit"
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
        Line::from("d                  Diagnostics browser"),
        Line::from("v                  Open the terminal UML diagram"),
        Line::from("Ctrl+U / Ctrl+D    Scroll type details"),
        Line::from("?                  Toggle this help"),
        Line::from("q                  Quit from the browser"),
        Line::from("Ctrl+C             Quit from any view"),
        Line::from(""),
        Line::from("Diagram: arrows/hjkl pan; Tab/BackTab or n/p focus; c center."),
        Line::from("Diagram: Enter opens details; Esc/b/v returns to the browser."),
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
    use crate::test_support::fixture_codebase;

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
        let text = render_text(&mut app, 20, 6);
        assert_eq!(text.lines().count(), 6);
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
}
