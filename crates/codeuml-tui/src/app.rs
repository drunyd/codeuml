use codeuml_core::{
    Codebase, Diagnostic, Provenance, RelationshipEvidence, RelationshipKind, ResolutionStatus,
    SymbolId, TypeDecl,
};
use codeuml_diagram::{
    project, DiagramEndpoint, DiagramNode, DiagramProjection, DiagramScope, NeighborhoodDirection,
    ProjectionError,
};

use crate::format;
use crate::terminal_diagram::{
    layout_terminal_diagram, render_diagram_canvas, DiagramLayoutOptions, TerminalCanvas,
    TerminalDiagramLayout,
};

const PAGE_STEP: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Browse,
    Diagram,
    Search,
    Diagnostics,
    ScopeDialog,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Focus {
    Types,
    Relationships,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    NavigateUp,
    NavigateDown,
    PageUp,
    PageDown,
    Home,
    End,
    Confirm,
    ToggleFocus,
    StartSearch,
    SearchInput(char),
    SearchBackspace,
    SearchSubmit,
    SearchCancel,
    OpenScope,
    Cancel,
    OpenDiagnostics,
    OpenHelp,
    ScrollDetailsUp,
    ScrollDetailsDown,
    OpenDiagram,
    PanUp,
    PanDown,
    PanLeft,
    PanRight,
    SelectPrevious,
    SelectNext,
    CenterDiagram,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RelationshipDirection {
    Outgoing,
    Incoming,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelationshipRow {
    pub direction: RelationshipDirection,
    pub kind: RelationshipKind,
    pub endpoint: DiagramEndpoint,
    pub resolution: ResolutionStatus,
    pub provenance: Provenance,
    pub evidence: Vec<RelationshipEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ScopeChoice {
    Project,
    Module { module: SymbolId, label: String },
    Neighborhood { type_id: SymbolId, label: String },
}

impl ScopeChoice {
    fn scope(&self) -> DiagramScope {
        match self {
            Self::Project => DiagramScope::Project,
            Self::Module { module, .. } => DiagramScope::Module {
                module: module.clone(),
            },
            Self::Neighborhood { type_id, .. } => DiagramScope::TypeNeighborhood {
                type_id: type_id.clone(),
                direction: NeighborhoodDirection::Both,
            },
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Project => "Project",
            Self::Module { label, .. } | Self::Neighborhood { label, .. } => label,
        }
    }

    fn matches_scope(&self, scope: &DiagramScope) -> bool {
        match (self, scope) {
            (Self::Project, DiagramScope::Project) => true,
            (
                Self::Module { module, .. },
                DiagramScope::Module {
                    module: selected, ..
                },
            ) => module == selected,
            (
                Self::Neighborhood { type_id, .. },
                DiagramScope::TypeNeighborhood {
                    type_id: selected, ..
                },
            ) => type_id == selected,
            _ => false,
        }
    }
}

pub(crate) struct App<'a> {
    codebase: &'a Codebase,
    projection: DiagramProjection,
    diagnostics: Vec<Diagnostic>,
    visible: Vec<usize>,
    selected: usize,
    mode: Mode,
    focus: Focus,
    search_query: String,
    search_backup: String,
    search_backup_selection: Option<SymbolId>,
    relationship_selected: usize,
    detail_scroll: u16,
    diagnostic_selected: usize,
    scope_choices: Vec<ScopeChoice>,
    scope_choice_selected: usize,
    status: String,
    should_quit: bool,
    overlay_return: Mode,
    diagram_options: DiagramLayoutOptions,
    diagram_layout: TerminalDiagramLayout,
    diagram_canvas: TerminalCanvas,
    diagram_pan_x: u16,
    diagram_pan_y: u16,
    diagram_viewport_width: u16,
    diagram_viewport_height: u16,
}

impl<'a> App<'a> {
    pub(crate) fn new(codebase: &'a Codebase) -> Result<Self, ProjectionError> {
        let projection = project(codebase, &DiagramScope::Project)?;
        let diagnostics = codebase.diagnostics().to_vec();
        let mut app = Self {
            codebase,
            projection,
            diagnostics,
            visible: Vec::new(),
            selected: 0,
            mode: Mode::Browse,
            focus: Focus::Types,
            search_query: String::new(),
            search_backup: String::new(),
            search_backup_selection: None,
            relationship_selected: 0,
            detail_scroll: 0,
            diagnostic_selected: 0,
            scope_choices: Vec::new(),
            scope_choice_selected: 0,
            status: String::new(),
            should_quit: false,
            overlay_return: Mode::Browse,
            diagram_options: DiagramLayoutOptions::default(),
            diagram_layout: TerminalDiagramLayout::default(),
            diagram_canvas: TerminalCanvas::default(),
            diagram_pan_x: 0,
            diagram_pan_y: 0,
            diagram_viewport_width: 0,
            diagram_viewport_height: 0,
        };
        app.rebuild_visible(None);
        app.rebuild_diagram_layout();
        app.status = if app.visible.is_empty() {
            "No types in project.".to_owned()
        } else {
            "Ready. Press ? for help.".to_owned()
        };
        Ok(app)
    }

    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn focus(&self) -> Focus {
        self.focus
    }

    pub(crate) fn projection(&self) -> &DiagramProjection {
        &self.projection
    }

    pub(crate) fn current_scope(&self) -> &DiagramScope {
        &self.projection.scope
    }

    pub(crate) fn visible_type_count(&self) -> usize {
        self.visible.len()
    }

    pub(crate) fn selected_type_id(&self) -> Option<&SymbolId> {
        self.selected_node().map(|node| &node.id)
    }

    pub(crate) fn selected_node(&self) -> Option<&DiagramNode> {
        self.visible
            .get(self.selected)
            .and_then(|index| self.projection.nodes.get(*index))
    }

    pub(crate) fn search_query(&self) -> &str {
        &self.search_query
    }

    pub(crate) fn status(&self) -> &str {
        &self.status
    }

    pub(crate) fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub(crate) fn diagnostic_selected(&self) -> usize {
        self.diagnostic_selected
    }

    pub(crate) fn relationship_selected(&self) -> usize {
        self.relationship_selected
    }

    pub(crate) fn detail_scroll(&self) -> u16 {
        self.detail_scroll
    }

    pub(crate) fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub(crate) fn current_scope_label(&self) -> String {
        match self.current_scope() {
            DiagramScope::Project => "Project".to_owned(),
            DiagramScope::Module { module } => format!(
                "Module: {}",
                self.codebase
                    .module(module)
                    .map(|module| module.qualified_name.as_str())
                    .unwrap_or(module.as_str())
            ),
            DiagramScope::TypeNeighborhood { type_id, .. } => format!(
                "Neighborhood: {}",
                self.codebase
                    .type_by_id(type_id)
                    .map(|declaration| declaration.name.as_str())
                    .unwrap_or(type_id.as_str())
            ),
        }
    }

    pub(crate) fn detail_lines(&self) -> Vec<String> {
        let Some(declaration) = self.selected_declaration() else {
            return vec!["No type selected.".to_owned()];
        };
        format::detail_lines(self.codebase, declaration)
    }

    fn selected_declaration(&self) -> Option<&'a TypeDecl> {
        self.selected_type_id()
            .and_then(|id| self.codebase.type_by_id(id))
    }

    pub(crate) fn set_scope(&mut self, scope: DiagramScope) -> Result<(), ProjectionError> {
        let keep = self.selected_type_id().cloned();
        self.replace_projection(scope, keep)?;
        self.focus = Focus::Types;
        self.relationship_selected = 0;
        self.detail_scroll = 0;
        self.status = format!("Scope: {}", self.current_scope_label());
        Ok(())
    }

    fn replace_projection(
        &mut self,
        scope: DiagramScope,
        keep: Option<SymbolId>,
    ) -> Result<(), ProjectionError> {
        let projection = project(self.codebase, &scope)?;
        self.projection = projection;
        self.rebuild_visible(keep);
        self.rebuild_diagram_layout();
        if self.mode == Mode::Diagram {
            self.center_diagram_on_selected();
        }
        Ok(())
    }

    fn rebuild_visible(&mut self, keep: Option<SymbolId>) {
        let query = self.search_query.to_lowercase();
        self.visible = self
            .projection
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                query.is_empty()
                    || node.name.to_lowercase().contains(&query)
                    || node.qualified_name.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect();

        self.selected = keep
            .and_then(|id| {
                self.visible.iter().position(|index| {
                    self.projection
                        .nodes
                        .get(*index)
                        .is_some_and(|node| node.id == id)
                })
            })
            .unwrap_or(0);
        self.relationship_selected = 0;
        self.detail_scroll = 0;

        if self.visible.is_empty() {
            self.status = if self.search_query.is_empty() {
                "No types in scope.".to_owned()
            } else {
                format!("No types match `{}`.", self.search_query)
            };
        }
    }

    pub(crate) fn handle(&mut self, action: Action) {
        if action == Action::Quit {
            self.should_quit = true;
            return;
        }

        match self.mode {
            Mode::Browse => self.handle_browse(action),
            Mode::Diagram => self.handle_diagram(action),
            Mode::Search => self.handle_search(action),
            Mode::Diagnostics => self.handle_diagnostics(action),
            Mode::ScopeDialog => self.handle_scope_dialog(action),
            Mode::Help => self.handle_help(action),
        }
    }

    fn handle_browse(&mut self, action: Action) {
        match action {
            Action::NavigateUp => self.navigate(-1),
            Action::NavigateDown => self.navigate(1),
            Action::PageUp => self.navigate(-(PAGE_STEP as isize)),
            Action::PageDown => self.navigate(PAGE_STEP as isize),
            Action::Home => self.navigate_to_boundary(false),
            Action::End => self.navigate_to_boundary(true),
            Action::ToggleFocus => self.toggle_focus(),
            Action::Confirm => {
                if self.focus == Focus::Relationships {
                    self.follow_selected_relationship();
                }
            }
            Action::StartSearch => self.start_search(),
            Action::SearchCancel => self.cancel_search(),
            Action::OpenScope => self.open_scope_dialog(),
            Action::Cancel => self.cancel_or_clear_search(),
            Action::OpenDiagnostics => self.open_diagnostics(),
            Action::OpenHelp => self.open_help(),
            Action::ScrollDetailsUp => self.scroll_details(-1),
            Action::ScrollDetailsDown => self.scroll_details(1),
            Action::OpenDiagram => self.open_diagram(),
            _ => {}
        }
    }

    fn handle_diagram(&mut self, action: Action) {
        match action {
            Action::PanUp | Action::NavigateUp => self.pan_diagram(0, -1),
            Action::PanDown | Action::NavigateDown => self.pan_diagram(0, 1),
            Action::PanLeft => self.pan_diagram(-1, 0),
            Action::PanRight => self.pan_diagram(1, 0),
            Action::PageUp => {
                let page = i32::from(self.diagram_viewport_height.saturating_sub(1).max(1));
                self.pan_diagram(0, -page);
            }
            Action::PageDown => {
                let page = i32::from(self.diagram_viewport_height.saturating_sub(1).max(1));
                self.pan_diagram(0, page);
            }
            Action::SelectNext => self.select_diagram_node(1),
            Action::SelectPrevious => self.select_diagram_node(-1),
            Action::CenterDiagram => {
                self.center_diagram_on_selected();
                self.status = "Diagram centered on focused type.".to_owned();
            }
            Action::Confirm => self.close_diagram(),
            Action::Cancel => self.close_diagram(),
            Action::OpenScope => self.open_scope_dialog(),
            Action::OpenDiagnostics => self.open_diagnostics(),
            Action::OpenHelp => self.open_help(),
            _ => {}
        }
    }

    fn handle_search(&mut self, action: Action) {
        match action {
            Action::SearchInput(character) => {
                let keep = self.selected_type_id().cloned();
                self.search_query.push(character);
                self.rebuild_visible(keep);
            }
            Action::SearchBackspace => {
                let keep = self.selected_type_id().cloned();
                self.search_query.pop();
                self.rebuild_visible(keep);
            }
            Action::SearchSubmit => {
                self.mode = Mode::Browse;
                self.status = if self.search_query.is_empty() {
                    "Search cleared.".to_owned()
                } else {
                    format!("Search: {}", self.search_query)
                };
            }
            Action::SearchCancel | Action::Cancel => self.cancel_search(),
            _ => {}
        }
    }

    fn handle_diagnostics(&mut self, action: Action) {
        match action {
            Action::NavigateUp => self.move_diagnostic(-1),
            Action::NavigateDown => self.move_diagnostic(1),
            Action::PageUp => self.move_diagnostic(-(PAGE_STEP as isize)),
            Action::PageDown => self.move_diagnostic(PAGE_STEP as isize),
            Action::Home => self.diagnostic_selected = 0,
            Action::End => {
                self.diagnostic_selected = self.diagnostics.len().saturating_sub(1);
            }
            Action::Cancel | Action::OpenDiagnostics => self.mode = self.overlay_return,
            _ => {}
        }
    }

    fn handle_scope_dialog(&mut self, action: Action) {
        match action {
            Action::NavigateUp => self.move_scope_choice(-1),
            Action::NavigateDown => self.move_scope_choice(1),
            Action::PageUp => self.move_scope_choice(-(PAGE_STEP as isize)),
            Action::PageDown => self.move_scope_choice(PAGE_STEP as isize),
            Action::Home => self.scope_choice_selected = 0,
            Action::End => {
                self.scope_choice_selected = self.scope_choices.len().saturating_sub(1);
            }
            Action::Confirm => {
                if let Some(choice) = self.scope_choices.get(self.scope_choice_selected).cloned() {
                    let return_mode = self.overlay_return;
                    self.mode = return_mode;
                    if let Err(error) = self.set_scope(choice.scope()) {
                        self.status = format!("Cannot switch scope: {error}");
                    } else if return_mode == Mode::Diagram {
                        self.center_diagram_on_selected();
                    }
                }
            }
            Action::Cancel => self.mode = self.overlay_return,
            _ => {}
        }
    }

    fn handle_help(&mut self, action: Action) {
        if matches!(action, Action::Cancel | Action::OpenHelp) {
            self.mode = self.overlay_return;
        }
    }

    fn navigate(&mut self, delta: isize) {
        if self.focus == Focus::Relationships {
            let count = self.relationship_rows().len();
            self.move_relationship(delta, count);
        } else {
            self.move_type(delta);
        }
    }

    fn navigate_to_boundary(&mut self, end: bool) {
        if self.focus == Focus::Relationships {
            let count = self.relationship_rows().len();
            if count > 0 {
                self.relationship_selected = if end { count - 1 } else { 0 };
            }
        } else if !self.visible.is_empty() {
            self.selected = if end { self.visible.len() - 1 } else { 0 };
            self.relationship_selected = 0;
            self.detail_scroll = 0;
            self.status = self.selected_status();
        }
    }

    fn move_type(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let maximum = self.visible.len() - 1;
        let next = (self.selected as isize + delta).clamp(0, maximum as isize) as usize;
        if next != self.selected {
            self.selected = next;
            self.relationship_selected = 0;
            self.detail_scroll = 0;
            self.status = self.selected_status();
        }
    }

    fn selected_status(&self) -> String {
        self.selected_node()
            .map(|node| format!("Selected {}", node.qualified_name))
            .unwrap_or_else(|| "No type selected.".to_owned())
    }

    fn move_relationship(&mut self, delta: isize, count: usize) {
        if count == 0 {
            self.relationship_selected = 0;
            self.status = "No relationships for selected type.".to_owned();
            return;
        }
        self.relationship_selected =
            (self.relationship_selected as isize + delta).clamp(0, (count - 1) as isize) as usize;
    }

    fn move_diagnostic(&mut self, delta: isize) {
        if self.diagnostics.is_empty() {
            self.diagnostic_selected = 0;
            return;
        }
        self.diagnostic_selected = (self.diagnostic_selected as isize + delta)
            .clamp(0, (self.diagnostics.len() - 1) as isize)
            as usize;
    }

    fn move_scope_choice(&mut self, delta: isize) {
        if self.scope_choices.is_empty() {
            self.scope_choice_selected = 0;
            return;
        }
        self.scope_choice_selected = (self.scope_choice_selected as isize + delta)
            .clamp(0, (self.scope_choices.len() - 1) as isize)
            as usize;
    }

    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Types => Focus::Relationships,
            Focus::Relationships => Focus::Types,
        };
        if self.focus == Focus::Relationships && self.relationship_rows().is_empty() {
            self.status = "No relationships for selected type.".to_owned();
        }
    }

    fn start_search(&mut self) {
        self.search_backup = self.search_query.clone();
        self.search_backup_selection = self.selected_type_id().cloned();
        self.mode = Mode::Search;
        self.status = format!("Search: {}", self.search_query);
    }

    fn cancel_search(&mut self) {
        self.search_query = self.search_backup.clone();
        self.rebuild_visible(self.search_backup_selection.clone());
        self.mode = Mode::Browse;
        self.status = "Search cancelled.".to_owned();
    }

    fn cancel_or_clear_search(&mut self) {
        if self.search_query.is_empty() {
            return;
        }
        let keep = self.selected_type_id().cloned();
        self.search_query.clear();
        self.search_backup.clear();
        self.rebuild_visible(keep);
        self.status = "Search cleared.".to_owned();
    }

    fn open_scope_dialog(&mut self) {
        self.overlay_return = self.mode;
        self.scope_choices = self.build_scope_choices();
        self.scope_choice_selected = self
            .scope_choices
            .iter()
            .position(|choice| choice.matches_scope(&self.projection.scope))
            .unwrap_or(0);
        self.mode = Mode::ScopeDialog;
    }

    fn build_scope_choices(&self) -> Vec<ScopeChoice> {
        let mut choices = vec![ScopeChoice::Project];
        let mut modules: Vec<_> = self.codebase.modules().iter().collect();
        modules.sort_by(|left, right| {
            (&left.qualified_name, &left.id).cmp(&(&right.qualified_name, &right.id))
        });
        for module in modules {
            choices.push(ScopeChoice::Module {
                module: module.id.clone(),
                label: format!("Module: {}", module.qualified_name),
            });
        }
        if let Some(declaration) = self.selected_declaration() {
            choices.push(ScopeChoice::Neighborhood {
                type_id: declaration.id.clone(),
                label: format!("Neighborhood: {}", declaration.qualified_name),
            });
        }
        choices
    }

    fn open_help(&mut self) {
        self.overlay_return = self.mode;
        self.mode = Mode::Help;
    }

    fn open_diagnostics(&mut self) {
        self.overlay_return = self.mode;
        self.mode = Mode::Diagnostics;
        if self.diagnostics.is_empty() {
            self.diagnostic_selected = 0;
            self.status = "No diagnostics.".to_owned();
        } else {
            self.diagnostic_selected = self
                .diagnostic_selected
                .min(self.diagnostics.len().saturating_sub(1));
            self.status = format!("{} diagnostic(s).", self.diagnostics.len());
        }
    }

    fn scroll_details(&mut self, delta: isize) {
        let maximum = self.detail_lines().len().saturating_sub(1) as isize;
        self.detail_scroll = (self.detail_scroll as isize + delta).clamp(0, maximum) as u16;
    }

    pub(crate) fn relationship_rows(&self) -> Vec<RelationshipRow> {
        let Some(selected) = self.selected_type_id() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for edge in &self.projection.edges {
            if edge.source == DiagramEndpoint::Node(selected.clone()) {
                rows.push(RelationshipRow {
                    direction: RelationshipDirection::Outgoing,
                    kind: edge.kind,
                    endpoint: edge.target.clone(),
                    resolution: edge.resolution,
                    provenance: edge.provenance,
                    evidence: edge.evidence.clone(),
                });
            }
            if edge.target == DiagramEndpoint::Node(selected.clone()) {
                rows.push(RelationshipRow {
                    direction: RelationshipDirection::Incoming,
                    kind: edge.kind,
                    endpoint: edge.source.clone(),
                    resolution: edge.resolution,
                    provenance: edge.provenance,
                    evidence: edge.evidence.clone(),
                });
            }
        }
        rows.sort_by(|left, right| {
            (left.direction, &left.endpoint, left.kind, left.resolution).cmp(&(
                right.direction,
                &right.endpoint,
                right.kind,
                right.resolution,
            ))
        });
        rows
    }

    pub(crate) fn relationship_row_text(&self, row: &RelationshipRow) -> String {
        let direction = match row.direction {
            RelationshipDirection::Outgoing => "OUT",
            RelationshipDirection::Incoming => "IN ",
        };
        let arrow = match row.direction {
            RelationshipDirection::Outgoing => "->",
            RelationshipDirection::Incoming => "<-",
        };
        let label = format::endpoint_label(&self.projection, &row.endpoint);
        let state = match (&row.endpoint, row.resolution) {
            (DiagramEndpoint::Boundary { symbol: None, .. }, _) => "unresolved",
            (DiagramEndpoint::Boundary { .. }, _) => "boundary",
            (DiagramEndpoint::Node(_), ResolutionStatus::Unresolved) => "unresolved",
            (DiagramEndpoint::Node(_), ResolutionStatus::Resolved) => "resolved",
            (DiagramEndpoint::Node(_), ResolutionStatus::Inferred) => "inferred",
        };
        let evidence = row
            .evidence
            .first()
            .map(|evidence| {
                format!(
                    " @ {}:{}",
                    evidence.location.file.display(),
                    evidence.location.line
                )
            })
            .unwrap_or_default();
        format!(
            "{direction} {} [{state}] {arrow} {label}{evidence}",
            row.kind
        )
    }

    fn follow_selected_relationship(&mut self) {
        let rows = self.relationship_rows();
        let Some(row) = rows.get(self.relationship_selected) else {
            self.status = "No relationship selected.".to_owned();
            return;
        };
        match &row.endpoint {
            DiagramEndpoint::Node(id) => self.navigate_to_type(id.clone()),
            DiagramEndpoint::Boundary {
                symbol: Some(id), ..
            } if self.codebase.type_by_id(id).is_some() => {
                self.navigate_to_type(id.clone());
            }
            DiagramEndpoint::Boundary { name, .. } => {
                self.status = format!("Unresolved relationship: {name}. Cannot navigate.");
            }
        }
    }

    fn navigate_to_type(&mut self, id: SymbolId) {
        if let Some(position) = self.visible.iter().position(|index| {
            self.projection
                .nodes
                .get(*index)
                .is_some_and(|node| node.id == id)
        }) {
            self.selected = position;
            self.focus = Focus::Types;
            self.relationship_selected = 0;
            self.detail_scroll = 0;
            self.mode = Mode::Browse;
            self.status = format!("Opened {}.", self.selected_status_value());
            return;
        }

        self.search_query.clear();
        self.rebuild_visible(Some(id.clone()));
        if self.selected_type_id() == Some(&id) {
            self.focus = Focus::Types;
            self.mode = Mode::Browse;
            self.status = format!("Opened {}.", self.selected_status_value());
            return;
        }

        if self
            .replace_projection(DiagramScope::Project, Some(id.clone()))
            .is_ok()
            && self.selected_type_id() == Some(&id)
        {
            self.focus = Focus::Types;
            self.mode = Mode::Browse;
            self.status = format!("Opened {}.", self.selected_status_value());
            return;
        }

        self.status = "Could not navigate to relationship target.".to_owned();
    }

    fn selected_status_value(&self) -> String {
        self.selected_node()
            .map(|node| node.qualified_name.clone())
            .unwrap_or_else(|| "unknown type".to_owned())
    }

    fn rebuild_diagram_layout(&mut self) {
        self.diagram_layout = layout_terminal_diagram(&self.projection, &self.diagram_options);
        self.rebuild_diagram_canvas();
        self.clamp_diagram_pan();
    }

    fn rebuild_diagram_canvas(&mut self) {
        self.diagram_canvas = render_diagram_canvas(&self.diagram_layout, self.selected_type_id());
    }

    fn open_diagram(&mut self) {
        self.mode = Mode::Diagram;
        self.focus = Focus::Types;
        self.rebuild_diagram_canvas();
        if self.diagram_layout.nodes.is_empty() {
            self.status = "No types in the selected scope.".to_owned();
            return;
        }
        if self.selected_type_id().is_none() {
            if let Some(first) = self.diagram_layout.nodes.first() {
                let id = first.id.clone();
                self.select_diagram_type(id);
            }
        } else {
            self.center_diagram_on_selected();
        }
        self.status = format!(
            "Diagram: {} | {} node(s), {} edge(s) | Enter details, Esc browser",
            self.current_scope_label(),
            self.diagram_layout.nodes.len(),
            self.diagram_layout.edges.len()
        );
    }

    fn close_diagram(&mut self) {
        self.mode = Mode::Browse;
        self.focus = Focus::Types;
        self.status = self.selected_status();
    }

    fn select_diagram_node(&mut self, delta: isize) {
        if self.diagram_layout.nodes.is_empty() {
            self.status = "No types in the selected scope.".to_owned();
            return;
        }
        let Some(selected) = self.selected_type_id().cloned() else {
            let id = self.diagram_layout.nodes[0].id.clone();
            self.select_diagram_type(id);
            return;
        };
        let current = self
            .diagram_layout
            .nodes
            .iter()
            .position(|node| node.id == selected)
            .unwrap_or(0);
        let count = self.diagram_layout.nodes.len();
        let next = (current as isize + delta).rem_euclid(count as isize) as usize;
        let id = self.diagram_layout.nodes[next].id.clone();
        self.select_diagram_type(id);
    }

    fn select_diagram_type(&mut self, id: SymbolId) {
        if let Some(position) = self.visible.iter().position(|index| {
            self.projection
                .nodes
                .get(*index)
                .is_some_and(|node| node.id == id)
        }) {
            self.selected = position;
        } else {
            self.search_query.clear();
            self.search_backup.clear();
            self.rebuild_visible(Some(id));
        }
        self.relationship_selected = 0;
        self.detail_scroll = 0;
        self.rebuild_diagram_canvas();
        self.center_diagram_on_selected();
        self.status = self.selected_status();
    }

    fn pan_diagram(&mut self, dx: i32, dy: i32) {
        let max_x = self
            .diagram_layout
            .width
            .saturating_sub(self.diagram_viewport_width);
        let max_y = self
            .diagram_layout
            .height
            .saturating_sub(self.diagram_viewport_height);
        self.diagram_pan_x = (i32::from(self.diagram_pan_x) + dx).clamp(0, i32::from(max_x)) as u16;
        self.diagram_pan_y = (i32::from(self.diagram_pan_y) + dy).clamp(0, i32::from(max_y)) as u16;
        self.status = format!(
            "Diagram pan ({}, {}) | {} node(s)",
            self.diagram_pan_x,
            self.diagram_pan_y,
            self.diagram_layout.nodes.len()
        );
    }

    fn center_diagram_on_selected(&mut self) {
        let Some(id) = self.selected_type_id() else {
            return;
        };
        let Some(rect) = self.diagram_layout.node_rect(id) else {
            return;
        };
        if self.diagram_viewport_width == 0 || self.diagram_viewport_height == 0 {
            return;
        }
        let target_x = rect
            .x
            .saturating_add(rect.width / 2)
            .saturating_sub(self.diagram_viewport_width / 2);
        let target_y = rect
            .y
            .saturating_add(rect.height / 2)
            .saturating_sub(self.diagram_viewport_height / 2);
        let max_x = self
            .diagram_layout
            .width
            .saturating_sub(self.diagram_viewport_width);
        let max_y = self
            .diagram_layout
            .height
            .saturating_sub(self.diagram_viewport_height);
        self.diagram_pan_x = target_x.min(max_x);
        self.diagram_pan_y = target_y.min(max_y);
    }

    pub(crate) fn set_diagram_viewport(&mut self, width: u16, height: u16) {
        if width == self.diagram_viewport_width && height == self.diagram_viewport_height {
            return;
        }
        self.diagram_viewport_width = width;
        self.diagram_viewport_height = height;
        self.clamp_diagram_pan();
        if !self.selected_diagram_rect_visible() {
            self.center_diagram_on_selected();
        }
    }

    fn clamp_diagram_pan(&mut self) {
        if self.diagram_viewport_width == 0 || self.diagram_viewport_height == 0 {
            self.diagram_pan_x = 0;
            self.diagram_pan_y = 0;
            return;
        }
        let max_x = self
            .diagram_layout
            .width
            .saturating_sub(self.diagram_viewport_width);
        let max_y = self
            .diagram_layout
            .height
            .saturating_sub(self.diagram_viewport_height);
        self.diagram_pan_x = self.diagram_pan_x.min(max_x);
        self.diagram_pan_y = self.diagram_pan_y.min(max_y);
    }

    fn selected_diagram_rect_visible(&self) -> bool {
        let Some(id) = self.selected_type_id() else {
            return true;
        };
        let Some(rect) = self.diagram_layout.node_rect(id) else {
            return true;
        };
        let right = self
            .diagram_pan_x
            .saturating_add(self.diagram_viewport_width);
        let bottom = self
            .diagram_pan_y
            .saturating_add(self.diagram_viewport_height);
        rect.x < right
            && rect.right() > self.diagram_pan_x
            && rect.y < bottom
            && rect.bottom() > self.diagram_pan_y
    }

    pub(crate) fn overlay_return(&self) -> Mode {
        self.overlay_return
    }

    pub(crate) fn diagram_layout(&self) -> &TerminalDiagramLayout {
        &self.diagram_layout
    }

    pub(crate) fn diagram_canvas(&self) -> &TerminalCanvas {
        &self.diagram_canvas
    }

    pub(crate) fn diagram_pan(&self) -> (u16, u16) {
        (self.diagram_pan_x, self.diagram_pan_y)
    }

    pub(crate) fn visible_nodes(&self) -> Vec<&DiagramNode> {
        self.visible
            .iter()
            .filter_map(|index| self.projection.nodes.get(*index))
            .collect()
    }

    pub(crate) fn selected_visible_index(&self) -> Option<usize> {
        if self.visible.is_empty() {
            None
        } else {
            Some(self.selected)
        }
    }

    pub(crate) fn project_name(&self) -> &str {
        self.codebase.name()
    }

    pub(crate) fn analysis_label(&self) -> String {
        let metadata = self.codebase.analysis();
        format!(
            "{} ({} analyzed, {} failed, {} diagnostics)",
            metadata.mode,
            metadata.files_analyzed,
            metadata.files_failed,
            self.diagnostics.len()
        )
    }

    pub(crate) fn scope_choice_labels(&self) -> Vec<&str> {
        self.scope_choices.iter().map(ScopeChoice::label).collect()
    }

    pub(crate) fn scope_choice_selected(&self) -> usize {
        self.scope_choice_selected
    }
}

#[cfg(test)]
mod tests {
    use codeuml_core::SymbolId;
    use codeuml_diagram::DiagramScope;

    use super::*;
    use crate::test_support::{fixture_codebase, type_named};

    fn selected_name<'a>(app: &'a App<'_>) -> Option<&'a str> {
        app.selected_node().map(|node| node.name.as_str())
    }

    fn search(app: &mut App<'_>, query: &str) {
        app.handle(Action::StartSearch);
        for character in query.chars() {
            app.handle(Action::SearchInput(character));
        }
    }

    #[test]
    fn initial_state_is_deterministic_and_uses_project_scope() {
        let codebase = fixture_codebase();
        let app = App::new(&codebase).expect("initial projection");

        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.focus(), Focus::Types);
        assert_eq!(app.current_scope(), &DiagramScope::Project);
        assert_eq!(app.visible_type_count(), 5);
        assert_eq!(
            app.selected_type_id(),
            Some(&type_named(&codebase, "Alpha").id)
        );
        assert_eq!(selected_name(&app), Some("Alpha"));
        assert_eq!(app.projection().edges.len(), 5);
        assert_eq!(app.diagnostics().len(), 2);
        assert!(!app.should_quit());
        assert!(app
            .detail_lines()
            .iter()
            .any(|line| line.contains("Fields")));
    }

    #[test]
    fn navigation_clamps_at_boundaries_and_supports_page_home_end() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        app.handle(Action::NavigateUp);
        assert_eq!(selected_name(&app), Some("Alpha"));

        app.handle(Action::NavigateDown);
        assert_eq!(selected_name(&app), Some("Api"));
        app.handle(Action::End);
        assert_eq!(selected_name(&app), Some("Shape"));
        app.handle(Action::NavigateDown);
        assert_eq!(selected_name(&app), Some("Shape"));
        app.handle(Action::PageUp);
        assert_eq!(selected_name(&app), Some("Alpha"));
        app.handle(Action::PageDown);
        assert_eq!(selected_name(&app), Some("Shape"));
        app.handle(Action::Home);
        assert_eq!(selected_name(&app), Some("Alpha"));
    }

    #[test]
    fn search_is_case_insensitive_matches_qualified_names_and_restores_on_cancel() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        search(&mut app, "ALP");
        assert_eq!(app.mode(), Mode::Search);
        assert_eq!(app.visible_type_count(), 1);
        assert_eq!(selected_name(&app), Some("Alpha"));
        app.handle(Action::SearchSubmit);
        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(app.search_query(), "ALP");

        app.handle(Action::StartSearch);
        for _ in 0..3 {
            app.handle(Action::SearchBackspace);
        }
        for character in "api::".chars() {
            app.handle(Action::SearchInput(character));
        }
        assert_eq!(app.visible_type_count(), 1);
        assert_eq!(selected_name(&app), Some("Api"));
        app.handle(Action::SearchCancel);
        assert_eq!(app.search_query(), "ALP");
        assert_eq!(selected_name(&app), Some("Alpha"));

        app.handle(Action::Cancel);
        assert_eq!(app.search_query(), "");
        assert_eq!(app.visible_type_count(), 5);
    }

    #[test]
    fn empty_search_results_and_selection_preservation_are_explicit() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::NavigateDown);
        assert_eq!(selected_name(&app), Some("Api"));

        app.handle(Action::StartSearch);
        for character in "beta".chars() {
            app.handle(Action::SearchInput(character));
        }
        assert_eq!(app.visible_type_count(), 1);
        assert_eq!(selected_name(&app), Some("Beta"));

        for character in "zzz".chars() {
            app.handle(Action::SearchInput(character));
        }
        assert_eq!(app.visible_type_count(), 0);
        assert_eq!(app.selected_type_id(), None);
        assert!(app.status().contains("No types match"));

        for _ in 0..3 {
            app.handle(Action::SearchBackspace);
        }
        assert_eq!(app.visible_type_count(), 1);
        assert_eq!(selected_name(&app), Some("Beta"));
        app.handle(Action::SearchSubmit);
    }

    #[test]
    fn scope_switching_preserves_selection_when_possible_and_rejects_missing_targets() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        app.set_scope(DiagramScope::Module {
            module: type_named(&codebase, "Alpha").module.clone().unwrap(),
        })
        .expect("module scope");
        assert_eq!(app.visible_type_count(), 4);
        assert_eq!(selected_name(&app), Some("Alpha"));

        app.set_scope(DiagramScope::TypeNeighborhood {
            type_id: type_named(&codebase, "Alpha").id.clone(),
            direction: codeuml_diagram::NeighborhoodDirection::Both,
        })
        .expect("neighborhood");
        assert_eq!(app.visible_type_count(), 4);
        assert_eq!(selected_name(&app), Some("Alpha"));

        let before = app.projection().clone();
        let missing = SymbolId::declaration("src/missing.rs", "missing");
        let error = app
            .set_scope(DiagramScope::Module { module: missing })
            .expect_err("missing module");
        assert_eq!(app.projection(), &before);
        assert!(error.to_string().contains("not found"));

        let error = app
            .set_scope(DiagramScope::TypeNeighborhood {
                type_id: SymbolId::declaration("src/missing.rs", "missing"),
                direction: codeuml_diagram::NeighborhoodDirection::Both,
            })
            .expect_err("missing neighborhood type");
        assert_eq!(app.projection(), &before);
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn scope_dialog_offers_project_modules_and_neighborhood() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        app.handle(Action::OpenScope);
        assert_eq!(app.mode(), Mode::ScopeDialog);
        app.handle(Action::NavigateDown);
        app.handle(Action::Confirm);

        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(
            app.current_scope(),
            &DiagramScope::Module {
                module: SymbolId::declaration("src/api.rs", "api")
            }
        );
        assert_eq!(app.visible_type_count(), 1);
        assert_eq!(selected_name(&app), Some("Api"));
    }

    #[test]
    fn relationship_rows_show_both_directions_and_follow_resolved_neighbors() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        let rows = app.relationship_rows();
        assert_eq!(rows.len(), 5);
        assert!(rows
            .iter()
            .any(|row| row.direction == RelationshipDirection::Outgoing));
        assert!(rows
            .iter()
            .any(|row| row.direction == RelationshipDirection::Incoming));

        app.handle(Action::ToggleFocus);
        assert_eq!(app.focus(), Focus::Relationships);
        app.handle(Action::Confirm);
        assert_eq!(selected_name(&app), Some("Beta"));

        app.handle(Action::Home);
        assert_eq!(selected_name(&app), Some("Alpha"));
        app.handle(Action::ToggleFocus);
        app.handle(Action::End);
        assert_eq!(app.relationship_selected(), 4);
        app.handle(Action::Confirm);
        assert_eq!(selected_name(&app), Some("Api"));
    }

    #[test]
    fn unresolved_relationships_do_not_navigate_and_show_feedback() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        app.handle(Action::ToggleFocus);
        app.handle(Action::Home);
        let unresolved_index = app
            .relationship_rows()
            .iter()
            .position(|row| row.resolution == codeuml_core::ResolutionStatus::Unresolved)
            .expect("unresolved relationship");
        while app.relationship_selected() < unresolved_index {
            app.handle(Action::NavigateDown);
        }
        let selected_before = app.selected_type_id().cloned();
        app.handle(Action::Confirm);
        assert_eq!(app.selected_type_id(), selected_before.as_ref());
        assert!(app.status().contains("Unresolved"));
    }

    #[test]
    fn boundary_neighbors_can_be_opened_in_project_scope() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.set_scope(DiagramScope::Module {
            module: type_named(&codebase, "Api").module.clone().unwrap(),
        })
        .expect("api module");
        assert_eq!(selected_name(&app), Some("Api"));

        app.handle(Action::ToggleFocus);
        app.handle(Action::Confirm);
        assert_eq!(app.current_scope(), &DiagramScope::Project);
        assert_eq!(selected_name(&app), Some("Alpha"));
        assert!(app.status().contains("Opened"));
    }

    #[test]
    fn diagnostics_navigation_and_empty_state_are_handled() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        app.handle(Action::OpenDiagnostics);
        assert_eq!(app.mode(), Mode::Diagnostics);
        assert_eq!(app.diagnostic_selected(), 0);
        app.handle(Action::NavigateDown);
        assert_eq!(app.diagnostic_selected(), 1);
        app.handle(Action::End);
        assert_eq!(app.diagnostic_selected(), 1);
        app.handle(Action::NavigateDown);
        assert_eq!(app.diagnostic_selected(), 1);
        app.handle(Action::Home);
        assert_eq!(app.diagnostic_selected(), 0);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Browse);

        let empty = codeuml_core::Codebase::new("empty");
        let mut empty_app = App::new(&empty).expect("empty projection");
        empty_app.handle(Action::OpenDiagnostics);
        assert_eq!(empty_app.diagnostic_selected(), 0);
        assert!(empty_app.status().contains("No diagnostics"));
    }

    #[test]
    fn detail_lines_cover_structs_enums_traits_and_methods() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        let alpha = app.detail_lines().join("\n");
        assert!(alpha.contains("Alpha"));
        assert!(alpha.contains("Fields"));
        assert!(alpha.contains("beta"));
        assert!(alpha.contains("find(id: u64)"));
        assert!(alpha.contains("Generics"));

        app.handle(Action::End);
        let shape = app.detail_lines().join("\n");
        assert!(shape.contains("Variants"));
        assert!(shape.contains("Circle"));
        assert!(shape.contains("radius: f64"));

        app.handle(Action::Home);
        app.handle(Action::NavigateDown);
        app.set_scope(DiagramScope::Module {
            module: type_named(&codebase, "Gamma").module.clone().unwrap(),
        })
        .expect("module scope");
        app.handle(Action::NavigateUp);
        app.handle(Action::NavigateDown);
        while selected_name(&app) != Some("Gamma") {
            app.handle(Action::NavigateDown);
        }
        let gamma = app.detail_lines().join("\n");
        assert!(gamma.contains("Trait") || gamma.contains("trait"));
        assert!(gamma.contains("run"));
    }

    #[test]
    fn diagram_view_switches_focuses_types_and_opens_details() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        app.handle(Action::OpenDiagram);
        assert_eq!(app.mode(), Mode::Diagram);
        assert!(!app.diagram_layout().nodes.is_empty());
        assert_eq!(selected_name(&app), Some("Alpha"));

        app.handle(Action::SelectNext);
        assert_eq!(selected_name(&app), Some("Api"));
        app.handle(Action::SelectPrevious);
        assert_eq!(selected_name(&app), Some("Alpha"));
        app.handle(Action::SelectPrevious);
        assert_eq!(selected_name(&app), Some("Shape"));

        app.handle(Action::Confirm);
        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(selected_name(&app), Some("Shape"));
        assert!(app.detail_lines().iter().any(|line| line.contains("Shape")));

        app.handle(Action::OpenDiagram);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Browse);
    }

    #[test]
    fn diagram_panning_recentering_and_resize_are_bounded_and_keep_focus_visible() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenDiagram);
        app.set_diagram_viewport(24, 8);

        app.handle(Action::PanRight);
        app.handle(Action::PanDown);
        let panned = app.diagram_pan();
        assert!(panned.0 > 0 || panned.1 > 0);
        assert!(app.status().contains("Diagram pan"));

        app.handle(Action::CenterDiagram);
        assert!(app.status().contains("centered"));

        app.handle(Action::SelectNext);
        app.set_diagram_viewport(12, 5);
        assert_eq!(app.mode(), Mode::Diagram);
        assert_eq!(selected_name(&app), Some("Api"));

        let (pan_x, pan_y) = app.diagram_pan();
        let layout = app.diagram_layout();
        assert!(pan_x <= layout.width);
        assert!(pan_y <= layout.height);
    }

    #[test]
    fn diagram_overlays_return_to_the_diagram_and_scope_switch_preserves_view() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenDiagram);

        app.handle(Action::OpenHelp);
        assert_eq!(app.mode(), Mode::Help);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Diagram);

        app.handle(Action::OpenDiagnostics);
        assert_eq!(app.mode(), Mode::Diagnostics);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Diagram);

        app.handle(Action::OpenScope);
        assert_eq!(app.mode(), Mode::ScopeDialog);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Diagram);

        app.handle(Action::OpenScope);
        app.handle(Action::NavigateDown);
        app.handle(Action::Confirm);
        assert_eq!(app.mode(), Mode::Diagram);
        assert_eq!(
            app.current_scope(),
            &DiagramScope::Module {
                module: SymbolId::declaration("src/api.rs", "api")
            }
        );
        assert_eq!(app.diagram_layout().nodes.len(), 1);
    }

    #[test]
    fn diagram_selection_recovers_when_search_hides_the_focused_type() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::StartSearch);
        for character in "alpha".chars() {
            app.handle(Action::SearchInput(character));
        }
        app.handle(Action::SearchSubmit);
        assert_eq!(app.visible_type_count(), 1);

        app.handle(Action::OpenDiagram);
        app.handle(Action::SelectNext);
        assert_eq!(app.search_query(), "");
        assert_eq!(selected_name(&app), Some("Api"));
        assert_eq!(app.mode(), Mode::Diagram);
    }

    #[test]
    fn help_and_quit_transitions_are_explicit() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");

        app.handle(Action::OpenHelp);
        assert_eq!(app.mode(), Mode::Help);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Browse);

        app.handle(Action::ScrollDetailsDown);
        assert_eq!(app.detail_scroll(), 1);
        app.handle(Action::ScrollDetailsUp);
        assert_eq!(app.detail_scroll(), 0);

        app.handle(Action::Quit);
        assert!(app.should_quit());
    }
}
