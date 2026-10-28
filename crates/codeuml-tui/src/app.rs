use std::collections::BTreeMap;
use std::path::PathBuf;

use codeuml_core::{
    Codebase, Diagnostic, Provenance, RelationshipEvidence, RelationshipKind, ResolutionStatus,
    SourceLocation, SymbolId, TypeDecl,
};
use codeuml_diagram::{
    project, DiagramEndpoint, DiagramNode, DiagramProjection, DiagramScope, NeighborhoodDirection,
    ProjectionError,
};

use codeuml_metrics::Metrics;

use crate::architecture_diagram::build_architecture_diagram;
use crate::format::{self, DetailRow};
use crate::score::CrapScore;
use crate::source::{self, SourceView};
use crate::terminal_diagram::{
    layout_terminal_diagram, render_diagram_canvas, DiagramLayoutOptions, TerminalCanvas,
    TerminalDiagramLayout, TerminalNodeScore,
};

const PAGE_STEP: usize = 10;
const HISTORY_LIMIT: usize = 100;
const ARCHITECTURE_DIAGRAM_LIST_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Browse,
    Architecture,
    ArchitectureDiagram,
    Source,
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
    OpenArchitecture,
    OpenArchitectureDiagram,
    ToggleArchitectureFull,
    ToggleArchitectureSourcePanel,
    OpenTypeBrowser,
    OpenSource,
    Back,
    Ascend,
    Descend,
    PanUp,
    PanDown,
    PanLeft,
    PanRight,
    SelectPrevious,
    SelectNext,
    CenterDiagram,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HistoryEntry {
    scope: DiagramScope,
    selected: Option<SymbolId>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ArchitecturePreview {
    Children {
        title: String,
        rows: Vec<ArchitectureRow>,
    },
    Details {
        title: String,
        lines: Vec<DetailRow>,
    },
    Empty,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArchitectureRow {
    pub id: SymbolId,
    pub label: String,
    pub is_module: bool,
    pub crap: Option<CrapScore>,
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
    architecture_parent: Option<SymbolId>,
    architecture_selected: usize,
    architecture_visual_selected: Option<SymbolId>,
    architecture_full: bool,
    architecture_diagram_selectable: Vec<SymbolId>,
    architecture_diagram_layout: TerminalDiagramLayout,
    architecture_diagram_canvas: TerminalCanvas,
    architecture_diagram_pan_x: u16,
    architecture_diagram_pan_y: u16,
    architecture_diagram_viewport_width: u16,
    architecture_diagram_viewport_height: u16,
    architecture_source_panel: bool,
    architecture_source: Option<SourceView>,
    architecture_source_error: Option<String>,
    architecture_source_scroll: usize,
    project_root: Option<PathBuf>,
    metrics: Option<Metrics>,
    source: Option<SourceView>,
    source_scroll: usize,
    source_return: Mode,
    history: Vec<HistoryEntry>,
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
            architecture_parent: None,
            architecture_selected: 0,
            architecture_visual_selected: None,
            architecture_full: false,
            architecture_diagram_selectable: Vec::new(),
            architecture_diagram_layout: TerminalDiagramLayout::default(),
            architecture_diagram_canvas: TerminalCanvas::default(),
            architecture_diagram_pan_x: 0,
            architecture_diagram_pan_y: 0,
            architecture_diagram_viewport_width: 0,
            architecture_diagram_viewport_height: 0,
            architecture_source_panel: false,
            architecture_source: None,
            architecture_source_error: None,
            architecture_source_scroll: 0,
            project_root: None,
            metrics: None,
            source: None,
            source_scroll: 0,
            source_return: Mode::Browse,
            history: Vec::new(),
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
        self.detail_rows().into_iter().map(|row| row.text).collect()
    }

    pub(crate) fn detail_rows(&self) -> Vec<DetailRow> {
        let Some(declaration) = self.selected_declaration() else {
            return vec![DetailRow {
                text: "No type selected.".to_owned(),
                crap: None,
            }];
        };
        format::detail_rows(self.codebase, declaration, self.metrics.as_ref())
    }

    pub(crate) fn with_metrics(mut self, metrics: Option<Metrics>) -> Self {
        self.metrics = metrics;
        self
    }

    /// Looks up a CRAP score and classifies it; `None` when no report is loaded
    /// or it has nothing for the element.
    fn crap_score(&self, pick: impl FnOnce(&Metrics) -> Option<f64>) -> Option<CrapScore> {
        let metrics = self.metrics.as_ref()?;
        pick(metrics).map(|value| CrapScore::new(metrics.level(value), value))
    }

    pub(crate) fn type_crap(&self, qualified_name: &str) -> Option<CrapScore> {
        self.crap_score(|metrics| metrics.type_score(qualified_name))
    }

    fn architecture_diagram_scores(&self) -> BTreeMap<SymbolId, TerminalNodeScore> {
        let mut scores = BTreeMap::new();
        for module in self.codebase.modules() {
            if let Some(score) =
                self.crap_score(|metrics| metrics.module_score(&module.qualified_name))
            {
                scores.insert(
                    module.id.clone(),
                    TerminalNodeScore {
                        text: score.text(),
                        level: score.level,
                    },
                );
            }
        }
        for declaration in self.codebase.types() {
            if let Some(score) = self.type_crap(&declaration.qualified_name) {
                scores.insert(
                    declaration.id.clone(),
                    TerminalNodeScore {
                        text: score.text(),
                        level: score.level,
                    },
                );
            }
        }
        scores
    }

    /// One-line explanation of the colour bands, shown when scores are loaded.
    pub(crate) fn crap_legend(&self) -> Option<String> {
        let thresholds = self.metrics.as_ref()?.thresholds();
        Some(format!(
            "CRAP: green <= {}, yellow <= {}, red above",
            thresholds.good_max, thresholds.medium_max
        ))
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
            Mode::Architecture => self.handle_architecture(action),
            Mode::ArchitectureDiagram => self.handle_architecture_diagram(action),
            Mode::Source => self.handle_source(action),
            Mode::Diagram => self.handle_diagram(action),
            Mode::Search => self.handle_search(action),
            Mode::Diagnostics => self.handle_diagnostics(action),
            Mode::ScopeDialog => self.handle_scope_dialog(action),
            Mode::Help => self.handle_help(action),
        }
    }

    fn handle_browse(&mut self, action: Action) {
        match action {
            Action::NavigateUp | Action::NavigateDown | Action::PageUp | Action::PageDown => {
                self.handle_browse_navigation(action)
            }
            Action::Home | Action::End => self.handle_browse_boundary(action),
            Action::ToggleFocus => self.toggle_focus(),
            Action::Confirm => self.confirm_browse(),
            Action::StartSearch | Action::SearchCancel | Action::Cancel => {
                self.handle_browse_search(action)
            }
            Action::OpenScope | Action::OpenDiagnostics | Action::OpenHelp => {
                self.handle_browse_overlays(action)
            }
            Action::ScrollDetailsUp | Action::ScrollDetailsDown => {
                self.handle_browse_detail_scroll(action)
            }
            Action::OpenDiagram | Action::OpenArchitecture | Action::OpenSource | Action::Back => {
                self.handle_browse_navigation_commands(action)
            }
            _ => {}
        }
    }

    fn handle_browse_navigation(&mut self, action: Action) {
        if let Some(delta) = list_navigation_delta(action) {
            self.navigate(delta);
        }
    }

    fn handle_browse_boundary(&mut self, action: Action) {
        self.navigate_to_boundary(action == Action::End);
    }

    fn confirm_browse(&mut self) {
        if self.focus == Focus::Relationships {
            self.follow_selected_relationship();
        }
    }

    fn handle_browse_search(&mut self, action: Action) {
        match action {
            Action::StartSearch => self.start_search(),
            Action::SearchCancel => self.cancel_search(),
            Action::Cancel => self.cancel_or_clear_search(),
            _ => {}
        }
    }

    fn handle_browse_overlays(&mut self, action: Action) {
        match action {
            Action::OpenScope => self.open_scope_dialog(),
            Action::OpenDiagnostics => self.open_diagnostics(),
            Action::OpenHelp => self.open_help(),
            _ => {}
        }
    }

    fn handle_browse_detail_scroll(&mut self, action: Action) {
        match action {
            Action::ScrollDetailsUp => self.scroll_details(-1),
            Action::ScrollDetailsDown => self.scroll_details(1),
            _ => {}
        }
    }

    fn handle_browse_navigation_commands(&mut self, action: Action) {
        match action {
            Action::OpenDiagram => self.open_diagram(),
            Action::OpenArchitecture => self.open_architecture(),
            Action::OpenSource => self.open_source(),
            Action::Back => self.go_back(),
            _ => {}
        }
    }

    fn handle_architecture(&mut self, action: Action) {
        match action {
            Action::OpenSource => self.open_source(),
            Action::NavigateUp | Action::NavigateDown | Action::PageUp | Action::PageDown => {
                if let Some(delta) = list_navigation_delta(action) {
                    self.move_architecture_selection(delta);
                }
            }
            Action::Home => self.architecture_selected = 0,
            Action::End => {
                self.architecture_selected = self.architecture_rows().len().saturating_sub(1);
            }
            Action::Confirm | Action::Descend => self.open_architecture_selection(),
            Action::Ascend => self.ascend_architecture(),
            Action::Cancel => self.leave_architecture_level(),
            Action::OpenScope | Action::OpenDiagnostics | Action::OpenHelp => {
                self.handle_browse_overlays(action)
            }
            Action::OpenArchitectureDiagram => self.open_architecture_diagram(),
            Action::OpenTypeBrowser => self.open_type_browser(),
            _ => {}
        }
    }

    fn ascend_architecture(&mut self) {
        if self.architecture_parent.is_some() {
            self.leave_architecture_level();
        } else {
            self.status = "Already at the project root.".to_owned();
        }
    }

    fn handle_architecture_diagram(&mut self, action: Action) {
        match action {
            Action::NavigateUp => self.select_architecture_diagram_node(-1),
            Action::NavigateDown => self.select_architecture_diagram_node(1),
            Action::Home => self.select_architecture_diagram_index(0),
            Action::End => {
                let last = self.architecture_diagram_selectable.len().saturating_sub(1);
                self.select_architecture_diagram_index(last);
            }
            Action::PageUp => {
                let page = i32::from(
                    self.architecture_diagram_viewport_height
                        .saturating_sub(1)
                        .max(1),
                );
                self.pan_architecture_diagram(0, -page);
            }
            Action::PageDown => {
                let page = i32::from(
                    self.architecture_diagram_viewport_height
                        .saturating_sub(1)
                        .max(1),
                );
                self.pan_architecture_diagram(0, page);
            }
            Action::PanUp => self.pan_architecture_diagram(0, -1),
            Action::PanDown => self.pan_architecture_diagram(0, 1),
            Action::PanLeft => self.pan_architecture_diagram(-1, 0),
            Action::PanRight => self.pan_architecture_diagram(1, 0),
            Action::Confirm | Action::Descend => self.open_architecture_diagram_selection(),
            Action::Ascend => self.ascend_architecture_diagram(),
            Action::ToggleArchitectureFull => self.toggle_architecture_full(),
            Action::ToggleArchitectureSourcePanel => self.toggle_architecture_source_panel(),
            Action::ScrollDetailsUp => self.scroll_architecture_source(-1),
            Action::ScrollDetailsDown => self.scroll_architecture_source(1),
            Action::CenterDiagram => {
                self.center_architecture_diagram_on_selected();
                self.status = "Architecture view centered on focused item.".to_owned();
            }
            Action::Cancel => self.close_architecture_diagram(),
            Action::OpenSource => self.open_source(),
            Action::OpenDiagnostics | Action::OpenHelp => self.handle_browse_overlays(action),
            Action::OpenTypeBrowser => self.open_type_browser(),
            _ => {}
        }
    }

    pub(crate) fn start_default_view(&mut self) {
        self.architecture_parent = None;
        self.architecture_selected = 0;
        self.open_architecture_diagram();
        if self.architecture_diagram_layout.nodes.is_empty() {
            self.mode = Mode::Browse;
            self.status = self.selected_status();
        }
    }

    fn open_type_browser(&mut self) {
        self.mode = Mode::Browse;
        self.focus = Focus::Types;
        self.status = self.selected_status();
    }

    fn open_architecture_diagram(&mut self) {
        self.mode = Mode::ArchitectureDiagram;
        self.architecture_full = false;
        self.architecture_visual_selected = self
            .architecture_rows()
            .get(self.architecture_selected)
            .map(|row| row.id.clone())
            .or_else(|| self.selected_type_id().cloned());
        self.rebuild_architecture_diagram();
        self.center_architecture_diagram_on_selected();
        self.status = format!(
            "Architecture diagram: {} | j/k focus, l/Enter open, h up, t full",
            self.architecture_path_label()
        );
    }

    fn close_architecture_diagram(&mut self) {
        if let Some(selected) = self.architecture_visual_selected.clone() {
            if let Some(position) = self
                .architecture_rows()
                .iter()
                .position(|row| row.id == selected)
            {
                self.architecture_selected = position;
            }
        }
        self.mode = Mode::Architecture;
        self.status = format!("Architecture: {} item(s)", self.architecture_rows().len());
    }

    fn rebuild_architecture_diagram(&mut self) {
        let view = build_architecture_diagram(
            self.codebase,
            self.architecture_parent.as_ref(),
            self.architecture_full,
            ARCHITECTURE_DIAGRAM_LIST_LIMIT,
        );
        self.architecture_diagram_selectable = view.selectable;
        if self
            .architecture_visual_selected
            .as_ref()
            .is_none_or(|id| !self.architecture_diagram_selectable.contains(id))
        {
            self.architecture_visual_selected =
                self.architecture_diagram_selectable.first().cloned();
        }
        let mut options = self.diagram_options.clone();
        options.node_scores = self.architecture_diagram_scores();
        options.show_qualified = false;
        self.architecture_diagram_layout = layout_terminal_diagram(&view.projection, &options);
        self.architecture_diagram_canvas = render_diagram_canvas(
            &self.architecture_diagram_layout,
            self.architecture_visual_selected.as_ref(),
        );
        self.clamp_architecture_diagram_pan();
        self.refresh_architecture_source_panel();
    }

    fn select_architecture_diagram_node(&mut self, delta: isize) {
        if self.architecture_diagram_selectable.is_empty() {
            self.status = "No modules or types in this level.".to_owned();
            return;
        }
        let current = self
            .architecture_visual_selected
            .as_ref()
            .and_then(|id| {
                self.architecture_diagram_selectable
                    .iter()
                    .position(|candidate| candidate == id)
            })
            .unwrap_or(0);
        let count = self.architecture_diagram_selectable.len();
        let next = (current as isize + delta).rem_euclid(count as isize) as usize;
        self.select_architecture_diagram_index(next);
    }

    fn select_architecture_diagram_index(&mut self, index: usize) {
        if let Some(id) = self.architecture_diagram_selectable.get(index).cloned() {
            self.architecture_visual_selected = Some(id);
            self.architecture_diagram_canvas = render_diagram_canvas(
                &self.architecture_diagram_layout,
                self.architecture_visual_selected.as_ref(),
            );
            self.center_architecture_diagram_on_selected();
            self.refresh_architecture_source_panel();
            self.status = self.architecture_selected_status();
        }
    }

    fn architecture_selected_status(&self) -> String {
        self.architecture_visual_selected
            .as_ref()
            .and_then(|id| {
                self.codebase
                    .module(id)
                    .map(|module| format!("Selected module {}", module.qualified_name))
                    .or_else(|| {
                        self.codebase.type_by_id(id).map(|declaration| {
                            format!("Selected type {}", declaration.qualified_name)
                        })
                    })
            })
            .unwrap_or_else(|| "Nothing selected.".to_owned())
    }

    fn open_architecture_diagram_selection(&mut self) {
        let Some(id) = self.architecture_visual_selected.clone() else {
            self.status = "Nothing selected.".to_owned();
            return;
        };
        if self.codebase.module(&id).is_some() {
            self.architecture_parent = Some(id);
            self.architecture_full = false;
            self.architecture_visual_selected = None;
            self.rebuild_architecture_diagram();
            self.center_architecture_diagram_on_selected();
            self.status = format!("Opened {}.", self.architecture_path_label());
            return;
        }
        self.status = "Leaf type selected; press t to open the type browser.".to_owned();
    }

    fn ascend_architecture_diagram(&mut self) {
        if self.architecture_full {
            self.architecture_full = false;
            self.rebuild_architecture_diagram();
            self.center_architecture_diagram_on_selected();
            self.status = "Architecture level view.".to_owned();
            return;
        }
        let Some(current) = self.architecture_parent.clone() else {
            self.close_architecture_diagram();
            return;
        };
        let parent = self
            .codebase
            .module(&current)
            .and_then(|module| module.parent.clone());
        self.architecture_parent = parent;
        self.architecture_visual_selected = Some(current);
        self.rebuild_architecture_diagram();
        self.center_architecture_diagram_on_selected();
        self.status = format!("Returned to {}.", self.architecture_path_label());
    }

    fn toggle_architecture_full(&mut self) {
        self.architecture_full = !self.architecture_full;
        self.rebuild_architecture_diagram();
        self.center_architecture_diagram_on_selected();
        self.status = if self.architecture_full {
            "Architecture: full hierarchy".to_owned()
        } else {
            format!("Architecture level: {}", self.architecture_path_label())
        };
    }

    fn pan_architecture_diagram(&mut self, dx: i32, dy: i32) {
        let max_x = self
            .architecture_diagram_layout
            .width
            .saturating_sub(self.architecture_diagram_viewport_width);
        let max_y = self
            .architecture_diagram_layout
            .height
            .saturating_sub(self.architecture_diagram_viewport_height);
        self.architecture_diagram_pan_x =
            (i32::from(self.architecture_diagram_pan_x) + dx).clamp(0, i32::from(max_x)) as u16;
        self.architecture_diagram_pan_y =
            (i32::from(self.architecture_diagram_pan_y) + dy).clamp(0, i32::from(max_y)) as u16;
        self.status = format!(
            "Architecture pan ({}, {})",
            self.architecture_diagram_pan_x, self.architecture_diagram_pan_y
        );
    }

    fn center_architecture_diagram_on_selected(&mut self) {
        let Some(id) = self.architecture_visual_selected.as_ref() else {
            return;
        };
        let Some(rect) = self.architecture_diagram_layout.node_rect(id) else {
            return;
        };
        if self.architecture_diagram_viewport_width == 0
            || self.architecture_diagram_viewport_height == 0
        {
            return;
        }
        let target_x = rect
            .x
            .saturating_add(rect.width / 2)
            .saturating_sub(self.architecture_diagram_viewport_width / 2);
        let target_y = rect
            .y
            .saturating_add(rect.height / 2)
            .saturating_sub(self.architecture_diagram_viewport_height / 2);
        let max_x = self
            .architecture_diagram_layout
            .width
            .saturating_sub(self.architecture_diagram_viewport_width);
        let max_y = self
            .architecture_diagram_layout
            .height
            .saturating_sub(self.architecture_diagram_viewport_height);
        self.architecture_diagram_pan_x = target_x.min(max_x);
        self.architecture_diagram_pan_y = target_y.min(max_y);
    }

    pub(crate) fn set_architecture_diagram_viewport(&mut self, width: u16, height: u16) {
        if width == self.architecture_diagram_viewport_width
            && height == self.architecture_diagram_viewport_height
        {
            return;
        }
        self.architecture_diagram_viewport_width = width;
        self.architecture_diagram_viewport_height = height;
        self.clamp_architecture_diagram_pan();
        if !self.architecture_selected_rect_visible() {
            self.center_architecture_diagram_on_selected();
        }
    }

    fn clamp_architecture_diagram_pan(&mut self) {
        if self.architecture_diagram_viewport_width == 0
            || self.architecture_diagram_viewport_height == 0
        {
            self.architecture_diagram_pan_x = 0;
            self.architecture_diagram_pan_y = 0;
            return;
        }
        let max_x = self
            .architecture_diagram_layout
            .width
            .saturating_sub(self.architecture_diagram_viewport_width);
        let max_y = self
            .architecture_diagram_layout
            .height
            .saturating_sub(self.architecture_diagram_viewport_height);
        self.architecture_diagram_pan_x = self.architecture_diagram_pan_x.min(max_x);
        self.architecture_diagram_pan_y = self.architecture_diagram_pan_y.min(max_y);
    }

    fn architecture_selected_rect_visible(&self) -> bool {
        let Some(id) = self.architecture_visual_selected.as_ref() else {
            return true;
        };
        let Some(rect) = self.architecture_diagram_layout.node_rect(id) else {
            return true;
        };
        let right = self
            .architecture_diagram_pan_x
            .saturating_add(self.architecture_diagram_viewport_width);
        let bottom = self
            .architecture_diagram_pan_y
            .saturating_add(self.architecture_diagram_viewport_height);
        rect.x < right
            && rect.right() > self.architecture_diagram_pan_x
            && rect.y < bottom
            && rect.bottom() > self.architecture_diagram_pan_y
    }

    fn toggle_architecture_source_panel(&mut self) {
        self.architecture_source_panel = !self.architecture_source_panel;
        if self.architecture_source_panel {
            self.refresh_architecture_source_panel();
            self.status = if self.architecture_source_error.is_some() {
                "Architecture source panel opened with an error.".to_owned()
            } else {
                "Architecture source panel opened.".to_owned()
            };
        } else {
            self.status = "Architecture source panel closed.".to_owned();
        }
    }

    fn refresh_architecture_source_panel(&mut self) {
        if !self.architecture_source_panel {
            return;
        }
        let Some(root) = self.project_root.clone() else {
            self.architecture_source = None;
            self.architecture_source_error =
                Some("Source viewing is unavailable: project root unknown.".to_owned());
            return;
        };
        let Some((location, title)) = self.architecture_diagram_source_target() else {
            self.architecture_source = None;
            self.architecture_source_error =
                Some("Nothing with a source location is selected.".to_owned());
            return;
        };
        match source::load_source(&root, &location, title) {
            Ok(view) => {
                self.architecture_source_scroll = view.highlight.0.saturating_sub(3);
                self.architecture_source = Some(view);
                self.architecture_source_error = None;
            }
            Err(message) => {
                self.architecture_source = None;
                self.architecture_source_error = Some(message);
            }
        }
    }

    fn scroll_architecture_source(&mut self, delta: isize) {
        if !self.architecture_source_panel {
            return;
        }
        let last = self
            .architecture_source
            .as_ref()
            .map_or(0, |view| view.lines.len().saturating_sub(1));
        let next = (self.architecture_source_scroll as isize + delta).clamp(0, last as isize);
        self.architecture_source_scroll = next as usize;
    }

    pub(crate) fn architecture_source_panel_enabled(&self) -> bool {
        self.architecture_source_panel
    }

    pub(crate) fn architecture_source_view(&self) -> Option<&SourceView> {
        self.architecture_source.as_ref()
    }

    pub(crate) fn architecture_source_error(&self) -> Option<&str> {
        self.architecture_source_error.as_deref()
    }

    pub(crate) fn architecture_source_scroll(&self) -> usize {
        self.architecture_source_scroll
    }

    pub(crate) fn architecture_diagram_layout(&self) -> &TerminalDiagramLayout {
        &self.architecture_diagram_layout
    }

    pub(crate) fn architecture_diagram_canvas(&self) -> &TerminalCanvas {
        &self.architecture_diagram_canvas
    }

    pub(crate) fn architecture_diagram_pan(&self) -> (u16, u16) {
        (
            self.architecture_diagram_pan_x,
            self.architecture_diagram_pan_y,
        )
    }

    pub(crate) fn architecture_diagram_full(&self) -> bool {
        self.architecture_full
    }

    fn handle_diagram(&mut self, action: Action) {
        match action {
            Action::PanUp
            | Action::PanDown
            | Action::PanLeft
            | Action::PanRight
            | Action::NavigateUp
            | Action::NavigateDown => self.pan_diagram_action(action),
            Action::PageUp | Action::PageDown => self.page_diagram(action),
            Action::SelectNext => self.select_diagram_node(1),
            Action::SelectPrevious => self.select_diagram_node(-1),
            Action::CenterDiagram => self.center_diagram(),
            Action::Confirm | Action::Cancel => self.close_diagram(),
            Action::OpenSource => self.open_source(),
            Action::OpenScope | Action::OpenDiagnostics | Action::OpenHelp => {
                self.handle_browse_overlays(action)
            }
            _ => {}
        }
    }

    fn pan_diagram_action(&mut self, action: Action) {
        match action {
            Action::PanUp | Action::NavigateUp => self.pan_diagram(0, -1),
            Action::PanDown | Action::NavigateDown => self.pan_diagram(0, 1),
            Action::PanLeft => self.pan_diagram(-1, 0),
            Action::PanRight => self.pan_diagram(1, 0),
            _ => {}
        }
    }

    fn page_diagram(&mut self, action: Action) {
        let page = i32::from(self.diagram_viewport_height.saturating_sub(1).max(1));
        let direction = if action == Action::PageUp {
            -page
        } else {
            page
        };
        self.pan_diagram(0, direction);
    }

    fn center_diagram(&mut self) {
        self.center_diagram_on_selected();
        self.status = "Diagram centered on focused type.".to_owned();
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
            Action::NavigateUp | Action::NavigateDown | Action::PageUp | Action::PageDown => {
                if let Some(delta) = list_navigation_delta(action) {
                    self.move_scope_choice(delta);
                }
            }
            Action::Home => self.scope_choice_selected = 0,
            Action::End => {
                self.scope_choice_selected = self.scope_choices.len().saturating_sub(1);
            }
            Action::Confirm => self.confirm_scope_choice(),
            Action::Cancel => self.mode = self.overlay_return,
            _ => {}
        }
    }

    fn confirm_scope_choice(&mut self) {
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

    fn handle_help(&mut self, action: Action) {
        if matches!(action, Action::Cancel | Action::OpenHelp) {
            self.mode = self.overlay_return;
        }
    }

    fn move_architecture_selection(&mut self, delta: isize) {
        let count = self.architecture_rows().len();
        if count == 0 {
            self.architecture_selected = 0;
            return;
        }
        self.architecture_selected =
            (self.architecture_selected as isize + delta).clamp(0, (count - 1) as isize) as usize;
    }

    fn open_architecture(&mut self) {
        let selected = self.selected_type_id().cloned();
        self.architecture_parent = match (&self.projection.scope, selected.as_ref()) {
            (DiagramScope::Module { module }, Some(id))
                if self
                    .codebase
                    .type_by_id(id)
                    .is_some_and(|declaration| declaration.module.as_ref() == Some(module)) =>
            {
                Some(module.clone())
            }
            _ => None,
        };
        self.architecture_selected = selected
            .and_then(|id| self.architecture_rows().iter().position(|row| row.id == id))
            .unwrap_or(0);
        self.mode = Mode::Architecture;
        self.status = format!("Architecture: {} item(s)", self.architecture_rows().len());
    }

    fn open_architecture_selection(&mut self) {
        let Some(row) = self
            .architecture_rows()
            .get(self.architecture_selected)
            .cloned()
        else {
            self.status = "No modules or types in this location.".to_owned();
            return;
        };
        if row.is_module {
            self.architecture_parent = Some(row.id.clone());
            self.architecture_selected = 0;
            self.status = format!("Opened module {}.", row.label);
            return;
        }

        let Some(declaration) = self.codebase.type_by_id(&row.id) else {
            self.status = "Selected type is no longer available.".to_owned();
            return;
        };
        let id = declaration.id.clone();
        let scope = declaration
            .module
            .clone()
            .map(|module| DiagramScope::Module { module })
            .unwrap_or(DiagramScope::Project);
        if let Err(error) = self.replace_projection(scope, Some(id.clone())) {
            self.status = format!("Cannot open type: {error}");
            return;
        }
        self.mode = Mode::Browse;
        self.focus = Focus::Types;
        self.status = format!("Opened {}.", self.selected_status_value());
    }

    fn leave_architecture_level(&mut self) {
        let Some(current) = self.architecture_parent.clone() else {
            self.mode = Mode::Browse;
            self.status = self.selected_status();
            return;
        };
        let Some(module) = self.codebase.module(&current) else {
            self.architecture_parent = None;
            self.architecture_selected = 0;
            self.status = "Returned to project architecture.".to_owned();
            return;
        };
        self.architecture_parent = module.parent.clone();
        self.architecture_selected = self
            .architecture_rows()
            .iter()
            .position(|row| row.id == current)
            .unwrap_or(0);
        self.status = format!("Returned to {}.", self.architecture_path_label());
    }

    pub(crate) fn architecture_rows(&self) -> Vec<ArchitectureRow> {
        self.rows_for(self.architecture_parent.as_ref())
    }

    fn rows_for(&self, parent: Option<&SymbolId>) -> Vec<ArchitectureRow> {
        let mut rows = Vec::new();
        for module in self
            .codebase
            .modules()
            .iter()
            .filter(|module| module.parent.as_ref() == parent)
        {
            rows.push(ArchitectureRow {
                id: module.id.clone(),
                label: format!("{}/", module.name),
                is_module: true,
                crap: self.crap_score(|metrics| metrics.module_score(&module.qualified_name)),
            });
        }
        for declaration in self
            .codebase
            .types()
            .iter()
            .filter(|declaration| declaration.module.as_ref() == parent)
        {
            rows.push(ArchitectureRow {
                id: declaration.id.clone(),
                label: format!("{}  [{}]", declaration.name, declaration.kind),
                is_module: false,
                crap: self.type_crap(&declaration.qualified_name),
            });
        }
        rows.sort_by(|left, right| {
            (!left.is_module, left.label.to_lowercase(), &left.id).cmp(&(
                !right.is_module,
                right.label.to_lowercase(),
                &right.id,
            ))
        });
        rows
    }

    /// The level above the current one with the current module selected, or
    /// `None` at the project root.
    pub(crate) fn architecture_parent_pane(&self) -> Option<(String, Vec<ArchitectureRow>, usize)> {
        let current = self.architecture_parent.as_ref()?;
        let grandparent = self
            .codebase
            .module(current)
            .and_then(|module| module.parent.clone());
        let rows = self.rows_for(grandparent.as_ref());
        let selected = rows.iter().position(|row| &row.id == current).unwrap_or(0);
        let title = grandparent
            .as_ref()
            .and_then(|id| self.codebase.module(id))
            .map(|module| module.qualified_name.clone())
            .unwrap_or_else(|| self.codebase.name().to_owned());
        Some((title, rows, selected))
    }

    pub(crate) fn architecture_preview(&self) -> ArchitecturePreview {
        let Some(row) = self
            .architecture_rows()
            .get(self.architecture_selected)
            .cloned()
        else {
            return ArchitecturePreview::Empty;
        };
        if row.is_module {
            let Some(module) = self.codebase.module(&row.id) else {
                return ArchitecturePreview::Empty;
            };
            ArchitecturePreview::Children {
                title: format!(
                    "{} ({}:{})",
                    module.qualified_name,
                    module.file.display(),
                    module.location.line
                ),
                rows: self.rows_for(Some(&module.id)),
            }
        } else {
            match self.codebase.type_by_id(&row.id) {
                Some(declaration) => ArchitecturePreview::Details {
                    title: declaration.qualified_name.clone(),
                    lines: format::detail_rows(self.codebase, declaration, self.metrics.as_ref()),
                },
                None => ArchitecturePreview::Empty,
            }
        }
    }

    pub(crate) fn architecture_path_label(&self) -> String {
        self.architecture_parent
            .as_ref()
            .and_then(|id| self.codebase.module(id))
            .map(|module| module.qualified_name.clone())
            .unwrap_or_else(|| self.codebase.name().to_owned())
    }

    pub(crate) fn architecture_selected(&self) -> usize {
        self.architecture_selected
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
        let previous = self.history_entry();
        if self.try_navigate_to_type(id) {
            if let Some(previous) = previous {
                if previous.selected.as_ref() != self.selected_type_id()
                    || previous.scope != self.projection.scope
                {
                    self.push_history(previous);
                }
            }
        } else {
            self.status = "Could not navigate to relationship target.".to_owned();
        }
    }

    fn history_entry(&self) -> Option<HistoryEntry> {
        self.selected_type_id().map(|id| HistoryEntry {
            scope: self.projection.scope.clone(),
            selected: Some(id.clone()),
        })
    }

    fn push_history(&mut self, entry: HistoryEntry) {
        if self.history.len() == HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.history.push(entry);
    }

    pub(crate) fn history_depth(&self) -> usize {
        self.history.len()
    }

    fn go_back(&mut self) {
        let Some(entry) = self.history.pop() else {
            self.status = "No earlier location to go back to.".to_owned();
            return;
        };
        if let Err(error) = self.replace_projection(entry.scope.clone(), entry.selected.clone()) {
            self.status = format!("Cannot go back: {error}");
            return;
        }
        if self.selected_type_id() != entry.selected.as_ref() {
            self.search_query.clear();
            self.search_backup.clear();
            self.rebuild_visible(entry.selected);
        }
        self.focus = Focus::Types;
        self.status = format!(
            "Back to {} ({} earlier).",
            self.selected_status_value(),
            self.history.len()
        );
    }

    fn try_navigate_to_type(&mut self, id: SymbolId) -> bool {
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
            return true;
        }

        self.search_query.clear();
        self.rebuild_visible(Some(id.clone()));
        if self.selected_type_id() == Some(&id) {
            self.focus = Focus::Types;
            self.mode = Mode::Browse;
            self.status = format!("Opened {}.", self.selected_status_value());
            return true;
        }

        if self
            .replace_projection(DiagramScope::Project, Some(id.clone()))
            .is_ok()
            && self.selected_type_id() == Some(&id)
        {
            self.focus = Focus::Types;
            self.mode = Mode::Browse;
            self.status = format!("Opened {}.", self.selected_status_value());
            return true;
        }

        false
    }

    pub(crate) fn with_project_root(mut self, root: PathBuf) -> Self {
        self.project_root = Some(root);
        self
    }

    pub(crate) fn source_view(&self) -> Option<&SourceView> {
        self.source.as_ref()
    }

    pub(crate) fn source_scroll(&self) -> usize {
        self.source_scroll
    }

    fn source_target(&self) -> Option<(SourceLocation, String)> {
        if self.mode == Mode::Architecture {
            return self.architecture_source_target();
        }
        if self.mode == Mode::ArchitectureDiagram {
            return self.architecture_diagram_source_target();
        }
        if self.mode == Mode::Browse && self.focus == Focus::Relationships {
            return self.relationship_source_target();
        }
        self.selected_source_target()
    }

    fn architecture_diagram_source_target(&self) -> Option<(SourceLocation, String)> {
        let id = self.architecture_visual_selected.as_ref()?;
        if let Some(module) = self.codebase.module(id) {
            Some((
                module.location.clone(),
                format!("Module {}", module.qualified_name),
            ))
        } else {
            let declaration = self.codebase.type_by_id(id)?;
            Some((
                declaration.location.clone(),
                declaration.qualified_name.clone(),
            ))
        }
    }

    fn architecture_source_target(&self) -> Option<(SourceLocation, String)> {
        let row = self
            .architecture_rows()
            .get(self.architecture_selected)
            .cloned()?;
        if row.is_module {
            let module = self.codebase.module(&row.id)?;
            Some((
                module.location.clone(),
                format!("Module {}", module.qualified_name),
            ))
        } else {
            let declaration = self.codebase.type_by_id(&row.id)?;
            Some((
                declaration.location.clone(),
                declaration.qualified_name.clone(),
            ))
        }
    }

    fn relationship_source_target(&self) -> Option<(SourceLocation, String)> {
        let rows = self.relationship_rows();
        let row = rows.get(self.relationship_selected)?;
        let evidence = row.evidence.first()?;
        Some((evidence.location.clone(), format!("Evidence: {}", row.kind)))
    }

    fn selected_source_target(&self) -> Option<(SourceLocation, String)> {
        let declaration = self.selected_declaration()?;
        Some((
            declaration.location.clone(),
            declaration.qualified_name.clone(),
        ))
    }

    fn open_source(&mut self) {
        let Some(root) = self.project_root.clone() else {
            self.status = "Source viewing is unavailable: project root unknown.".to_owned();
            return;
        };
        let Some((location, title)) = self.source_target() else {
            self.status = "Nothing with a source location is selected.".to_owned();
            return;
        };
        match source::load_source(&root, &location, title) {
            Ok(view) => {
                self.source_scroll = view.highlight.0.saturating_sub(3);
                self.status = format!("{}:{}", view.file, view.highlight.0 + 1);
                self.source = Some(view);
                self.source_return = self.mode;
                self.mode = Mode::Source;
            }
            Err(message) => self.status = message,
        }
    }

    fn close_source(&mut self) {
        self.mode = self.source_return;
        self.source = None;
        self.status = "Closed source view.".to_owned();
    }

    fn handle_source(&mut self, action: Action) {
        let last = self
            .source
            .as_ref()
            .map_or(0, |view| view.lines.len().saturating_sub(1));
        match action {
            Action::NavigateUp | Action::PanUp => {
                self.source_scroll = self.source_scroll.saturating_sub(1);
            }
            Action::NavigateDown | Action::PanDown => {
                self.source_scroll = (self.source_scroll + 1).min(last);
            }
            Action::PageUp => {
                self.source_scroll = self.source_scroll.saturating_sub(PAGE_STEP);
            }
            Action::PageDown => {
                self.source_scroll = (self.source_scroll + PAGE_STEP).min(last);
            }
            Action::Home => self.source_scroll = 0,
            Action::End => self.source_scroll = last,
            Action::Cancel | Action::OpenSource => self.close_source(),
            _ => {}
        }
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

fn list_navigation_delta(action: Action) -> Option<isize> {
    match action {
        Action::NavigateUp => Some(-1),
        Action::NavigateDown => Some(1),
        Action::PageUp => Some(-(PAGE_STEP as isize)),
        Action::PageDown => Some(PAGE_STEP as isize),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use codeuml_core::SymbolId;
    use codeuml_diagram::DiagramScope;

    use super::*;
    use crate::test_support::{fixture_codebase, type_named};

    #[test]
    fn crap_scores_aggregate_into_types_modules_and_details() {
        use codeuml_metrics::CrapLevel;

        let codebase = fixture_codebase();
        let metrics = crate::test_support::fixture_metrics();
        let mut app = App::new(&codebase)
            .expect("initial projection")
            .with_metrics(Some(metrics));

        let alpha = app.type_crap("models::Alpha").expect("alpha score");
        assert_eq!((alpha.value, alpha.level), (14.5, CrapLevel::Bad));
        assert!(app.type_crap("api::Api").is_none());

        let rows = app.detail_rows();
        assert_eq!(rows[0].crap.map(|score| score.value), Some(14.5));
        let method = |name: &str| {
            rows.iter()
                .find(|row| row.text.contains(name))
                .and_then(|row| row.crap)
        };
        assert_eq!(
            method("find(").map(|score| score.level),
            Some(CrapLevel::Bad)
        );
        assert_eq!(
            method("measure(").map(|score| score.level),
            Some(CrapLevel::Good)
        );
        assert!(rows
            .iter()
            .filter(|row| row.text.starts_with("Kind") || row.text.starts_with("  pub beta"))
            .all(|row| row.crap.is_none()));

        app.handle(Action::OpenArchitecture);
        let module = |app: &App<'_>, name: &str| {
            app.architecture_rows()
                .into_iter()
                .find(|row| row.label.starts_with(name))
                .and_then(|row| row.crap)
        };
        let api = module(&app, "api/").expect("api module score");
        assert_eq!((api.value, api.level), (8.0, CrapLevel::Medium));
        let models = module(&app, "models/").expect("models module score");
        assert_eq!((models.value, models.level), (14.5, CrapLevel::Bad));

        app.handle(Action::NavigateDown);
        app.handle(Action::Descend);
        assert_eq!(app.architecture_path_label(), "models");
        assert!(app
            .architecture_rows()
            .iter()
            .any(|row| !row.is_module && row.crap.is_some()));
        assert_eq!(
            app.crap_legend().as_deref(),
            Some("CRAP: green <= 6, yellow <= 12, red above")
        );
    }

    #[test]
    fn nothing_is_scored_without_loaded_metrics() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        assert!(app.type_crap("models::Alpha").is_none());
        assert!(app.crap_legend().is_none());
        assert!(app.detail_rows().iter().all(|row| row.crap.is_none()));
        app.handle(Action::OpenArchitecture);
        assert!(app.architecture_rows().iter().all(|row| row.crap.is_none()));
    }

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
    fn architecture_tree_drills_through_modules_and_opens_types_in_module_scope() {
        let mut codebase = fixture_codebase();
        let models = type_named(&codebase, "Alpha").module.clone().unwrap();
        let nested = SymbolId::declaration("src/models/nested.rs", "models::nested");
        codebase
            .add_module(
                codeuml_core::Module::new(
                    nested.clone(),
                    "nested",
                    "models::nested",
                    "src/models/nested.rs",
                    crate::test_support::location("src/models/nested.rs", 1),
                )
                .with_parent(models.clone()),
            )
            .expect("unique nested module");
        let nested_type =
            SymbolId::declaration("src/models/nested.rs", "models::nested::NestedType");
        codebase
            .add_type(
                TypeDecl::new(
                    nested_type.clone(),
                    "NestedType",
                    "models::nested::NestedType",
                    codeuml_core::TypeKind::Struct,
                    crate::test_support::location("src/models/nested.rs", 3),
                )
                .with_module(nested.clone()),
            )
            .expect("unique nested type");

        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenArchitecture);
        assert_eq!(app.mode(), Mode::Architecture);
        assert_eq!(app.architecture_rows().len(), 2);

        app.handle(Action::NavigateDown);
        app.handle(Action::Confirm);
        assert_eq!(app.architecture_path_label(), "models");
        assert!(app
            .architecture_rows()
            .iter()
            .any(|row| row.id == nested && row.is_module));

        app.handle(Action::Home);
        app.handle(Action::Confirm);
        assert_eq!(app.architecture_path_label(), "models::nested");
        assert_eq!(app.architecture_rows()[0].id, nested_type);
        app.handle(Action::Confirm);
        assert_eq!(app.mode(), Mode::Browse);
        assert_eq!(
            app.current_scope(),
            &DiagramScope::Module {
                module: nested.clone()
            }
        );
        assert_eq!(selected_name(&app), Some("NestedType"));

        app.handle(Action::OpenArchitecture);
        assert_eq!(app.architecture_path_label(), "models::nested");
        assert_eq!(
            app.architecture_rows()[app.architecture_selected()].id,
            nested_type
        );
        app.handle(Action::Cancel);
        assert_eq!(app.architecture_path_label(), "models");
        app.handle(Action::Cancel);
        assert_eq!(app.architecture_path_label(), "fixture-project");
        assert_eq!(app.architecture_rows()[1].id, models);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Browse);
    }

    #[test]
    fn architecture_ascend_descend_and_panes_track_the_selection() {
        let codebase = fixture_codebase();
        let models = type_named(&codebase, "Alpha").module.clone().unwrap();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenArchitecture);

        assert!(app.architecture_parent_pane().is_none());
        app.handle(Action::Ascend);
        assert_eq!(app.mode(), Mode::Architecture);
        assert!(app.status().contains("project root"));

        match app.architecture_preview() {
            ArchitecturePreview::Children { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert!(!rows[0].is_module);
            }
            other => panic!("unexpected preview {other:?}"),
        }

        app.handle(Action::NavigateDown);
        app.handle(Action::Descend);
        assert_eq!(app.architecture_path_label(), "models");
        let (title, parent_rows, selected) = app.architecture_parent_pane().expect("parent pane");
        assert_eq!(title, "fixture-project");
        assert_eq!(parent_rows.len(), 2);
        assert_eq!(parent_rows[selected].id, models);

        match app.architecture_preview() {
            ArchitecturePreview::Details { title, .. } => assert_eq!(title, "models::Alpha"),
            other => panic!("unexpected preview {other:?}"),
        }
        app.handle(Action::NavigateDown);
        match app.architecture_preview() {
            ArchitecturePreview::Details { title, .. } => assert_eq!(title, "models::Beta"),
            other => panic!("unexpected preview {other:?}"),
        }

        app.handle(Action::Ascend);
        assert_eq!(app.mode(), Mode::Architecture);
        assert_eq!(app.architecture_path_label(), "fixture-project");
        assert_eq!(
            app.architecture_rows()[app.architecture_selected()].id,
            models
        );
    }

    #[test]
    fn source_view_opens_declaration_and_relationship_evidence_and_scrolls_within_bounds() {
        let project = crate::test_support::fixture_project("source-view");
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase)
            .expect("initial projection")
            .with_project_root(project.0.clone());

        app.handle(Action::OpenSource);
        assert_eq!(app.mode(), Mode::Source);
        let view = app.source_view().expect("source view");
        assert_eq!(view.title, "models::Alpha");
        assert_eq!(view.highlight, (2, 2));
        assert_eq!(app.source_scroll(), 0);

        app.handle(Action::End);
        assert_eq!(app.source_scroll(), 39);
        app.handle(Action::NavigateDown);
        assert_eq!(app.source_scroll(), 39);
        app.handle(Action::Home);
        app.handle(Action::NavigateUp);
        assert_eq!(app.source_scroll(), 0);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Browse);
        assert!(app.source_view().is_none());

        app.handle(Action::ToggleFocus);
        app.handle(Action::OpenSource);
        assert_eq!(app.mode(), Mode::Source);
        assert!(app
            .source_view()
            .expect("evidence")
            .title
            .starts_with("Evidence:"));
        app.handle(Action::OpenSource);
        assert_eq!(app.mode(), Mode::Browse);

        app.handle(Action::OpenDiagram);
        app.handle(Action::OpenSource);
        assert_eq!(app.mode(), Mode::Source);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Diagram);

        app.handle(Action::Cancel);
        app.handle(Action::OpenArchitecture);
        app.handle(Action::OpenSource);
        assert_eq!(app.mode(), Mode::Source);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Architecture);
    }

    #[test]
    fn source_view_reports_missing_root_and_unreadable_files_without_changing_mode() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenSource);
        assert_eq!(app.mode(), Mode::Browse);
        assert!(app.status().contains("unavailable"));

        let empty = crate::test_support::TempDir::new("no-files");
        let mut app = App::new(&codebase)
            .expect("initial projection")
            .with_project_root(empty.0.clone());
        app.handle(Action::OpenSource);
        assert_eq!(app.mode(), Mode::Browse);
        assert!(app.status().contains("Cannot open"), "{}", app.status());
    }

    #[test]
    fn back_returns_through_followed_relationships_and_scope_changes() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::Back);
        assert!(app.status().contains("No earlier"));

        app.handle(Action::ToggleFocus);
        app.handle(Action::Confirm);
        assert_eq!(selected_name(&app), Some("Beta"));
        assert_eq!(app.history_depth(), 1);
        app.handle(Action::Back);
        assert_eq!(selected_name(&app), Some("Alpha"));
        assert_eq!(app.history_depth(), 0);

        app.set_scope(DiagramScope::Module {
            module: type_named(&codebase, "Api").module.clone().unwrap(),
        })
        .expect("api module");
        app.handle(Action::ToggleFocus);
        app.handle(Action::Confirm);
        assert_eq!(app.current_scope(), &DiagramScope::Project);
        assert_eq!(selected_name(&app), Some("Alpha"));
        app.handle(Action::Back);
        assert_eq!(selected_name(&app), Some("Api"));
        assert_eq!(
            app.current_scope(),
            &DiagramScope::Module {
                module: type_named(&codebase, "Api").module.clone().unwrap()
            }
        );
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
    fn architecture_diagram_level_full_zoom_back_and_type_opening_work() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenArchitecture);
        app.handle(Action::OpenArchitectureDiagram);

        assert_eq!(app.mode(), Mode::ArchitectureDiagram);
        assert!(!app.architecture_full);
        assert!(!app.architecture_diagram_layout.nodes.is_empty());
        assert_eq!(
            app.architecture_visual_selected.as_ref(),
            app.architecture_diagram_selectable.first()
        );

        app.handle(Action::NavigateDown);
        let models = SymbolId::declaration("src/models.rs", "models");
        assert_eq!(app.architecture_visual_selected.as_ref(), Some(&models));
        let models_rect = app
            .architecture_diagram_layout
            .node_rect(&models)
            .expect("models rect");
        assert_eq!(
            app.architecture_diagram_canvas
                .cell(models_rect.x + 1, models_rect.y + 3)
                .expect("rule cell")
                .ch,
            '─'
        );

        app.handle(Action::Confirm);
        assert_eq!(app.architecture_parent.as_ref(), Some(&models));
        assert_eq!(app.architecture_path_label(), "models");
        assert!(app
            .architecture_diagram_layout
            .nodes
            .iter()
            .any(|node| node.name == "Alpha"));
        assert_eq!(
            app.architecture_visual_selected.as_ref(),
            app.architecture_diagram_selectable.first()
        );

        app.handle(Action::ToggleArchitectureFull);
        assert!(app.architecture_full);
        assert!(app.architecture_diagram_layout.nodes.len() > 5);
        app.handle(Action::ToggleArchitectureFull);
        assert!(!app.architecture_full);

        app.handle(Action::Ascend);
        assert_eq!(app.architecture_parent, None);
        assert_eq!(app.architecture_visual_selected.as_ref(), Some(&models));
        app.handle(Action::Ascend);
        assert_eq!(app.mode(), Mode::Architecture);

        app.handle(Action::OpenArchitectureDiagram);
        app.handle(Action::NavigateDown);
        app.handle(Action::Confirm);
        app.handle(Action::Confirm);
        assert_eq!(app.mode(), Mode::ArchitectureDiagram);
        let selected = app.architecture_visual_selected.clone();
        app.handle(Action::Descend);
        assert_eq!(app.mode(), Mode::ArchitectureDiagram);
        assert_eq!(app.architecture_visual_selected, selected);
        assert!(app.status().contains("Leaf type"));
    }

    #[test]
    fn default_tui_view_is_architecture_map_and_t_switches_to_type_browser() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.start_default_view();
        assert_eq!(app.mode(), Mode::ArchitectureDiagram);
        assert!(!app.architecture_diagram_layout.nodes.is_empty());

        app.handle(Action::OpenTypeBrowser);
        assert_eq!(app.mode(), Mode::Browse);
        assert!(app.detail_lines().iter().any(|line| line.contains("Name:")));
    }

    #[test]
    fn architecture_diagram_scores_are_placed_top_right_with_crap_band_styles() {
        use crate::terminal_diagram::TerminalDiagramStyle;
        use codeuml_metrics::CrapLevel;

        let codebase = fixture_codebase();
        let metrics = crate::test_support::fixture_metrics();
        let mut app = App::new(&codebase)
            .expect("initial projection")
            .with_metrics(Some(metrics));
        app.handle(Action::OpenArchitecture);
        app.handle(Action::OpenArchitectureDiagram);

        let models = SymbolId::declaration("src/models.rs", "models");
        let api = SymbolId::declaration("src/api.rs", "api");
        let models_node = app
            .architecture_diagram_layout
            .nodes
            .iter()
            .find(|node| node.id == models)
            .expect("models node");
        assert_eq!(
            models_node.score.as_ref().map(|score| score.text.as_str()),
            Some("14.5")
        );
        assert_eq!(
            models_node.score.as_ref().map(|score| score.level),
            Some(CrapLevel::Bad)
        );
        let score_width = 4;
        let score_x = models_node.rect.right() - 2 - score_width;
        let score_cell = app
            .architecture_diagram_canvas
            .cell(score_x, models_node.rect.y + 1)
            .expect("score cell");
        assert_eq!(score_cell.style, TerminalDiagramStyle::ScoreBad);

        let api_node = app
            .architecture_diagram_layout
            .nodes
            .iter()
            .find(|node| node.id == api)
            .expect("api node");
        assert_eq!(
            api_node.score.as_ref().map(|score| score.text.as_str()),
            Some("8.0")
        );
        let api_score_x = api_node.rect.right() - 2 - 3;
        assert_eq!(
            app.architecture_diagram_canvas
                .cell(api_score_x, api_node.rect.y + 1)
                .expect("api score cell")
                .style,
            TerminalDiagramStyle::ScoreMedium
        );
    }

    #[test]
    fn architecture_diagram_score_colours_follow_configured_thresholds() {
        use crate::terminal_diagram::TerminalDiagramStyle;
        use codeuml_metrics::{CrapEntry, CrapLevel, CrapThresholds, Metrics};

        let codebase = fixture_codebase();
        let metrics = Metrics::new(
            &[
                CrapEntry {
                    name: "find".to_owned(),
                    namespace: "models::Alpha".to_owned(),
                    crap: 14.5,
                },
                CrapEntry {
                    name: "helper".to_owned(),
                    namespace: "api".to_owned(),
                    crap: 8.0,
                },
            ],
            CrapThresholds {
                good_max: 10.0,
                medium_max: 20.0,
            },
        );
        let mut app = App::new(&codebase)
            .expect("initial projection")
            .with_metrics(Some(metrics));
        app.handle(Action::OpenArchitecture);
        app.handle(Action::OpenArchitectureDiagram);

        let models = SymbolId::declaration("src/models.rs", "models");
        let api = SymbolId::declaration("src/api.rs", "api");
        let models_node = app
            .architecture_diagram_layout
            .nodes
            .iter()
            .find(|node| node.id == models)
            .expect("models node");
        let api_node = app
            .architecture_diagram_layout
            .nodes
            .iter()
            .find(|node| node.id == api)
            .expect("api node");

        assert_eq!(
            models_node.score.as_ref().map(|score| score.level),
            Some(CrapLevel::Medium)
        );
        assert_eq!(
            api_node.score.as_ref().map(|score| score.level),
            Some(CrapLevel::Good)
        );
        let models_score_x = models_node.rect.right() - 2 - 4;
        let api_score_x = api_node.rect.right() - 2 - 3;
        assert_eq!(
            app.architecture_diagram_canvas
                .cell(models_score_x, models_node.rect.y + 1)
                .expect("models score cell")
                .style,
            TerminalDiagramStyle::ScoreMedium
        );
        assert_eq!(
            app.architecture_diagram_canvas
                .cell(api_score_x, api_node.rect.y + 1)
                .expect("api score cell")
                .style,
            TerminalDiagramStyle::ScoreGood
        );
    }

    #[test]
    fn architecture_source_panel_tracks_the_focused_element_and_scrolls() {
        let project = crate::test_support::fixture_project("architecture-source-panel");
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase)
            .expect("initial projection")
            .with_project_root(project.0.clone());
        app.start_default_view();
        app.handle(Action::ToggleArchitectureSourcePanel);

        assert_eq!(app.mode(), Mode::ArchitectureDiagram);
        assert!(app.architecture_source_panel_enabled());
        let view = app.architecture_source_view().expect("module source");
        assert_eq!(view.title, "Module api");
        assert_eq!(view.file, "src/api.rs");
        assert_eq!(view.highlight, (0, 0));

        app.handle(Action::NavigateDown);
        assert_eq!(
            app.architecture_source_view()
                .map(|view| view.title.as_str()),
            Some("Module models")
        );

        app.handle(Action::Confirm);
        let view = app.architecture_source_view().expect("type source");
        assert_eq!(view.title, "models::Alpha");
        assert_eq!(view.file, "src/models.rs");
        assert_eq!(view.highlight, (2, 2));
        let title_before_leaf = view.title.clone();
        let scroll_before = app.architecture_source_scroll();

        app.handle(Action::ScrollDetailsDown);
        assert_eq!(app.architecture_source_scroll(), scroll_before + 1);
        app.handle(Action::Confirm);
        assert_eq!(
            app.architecture_source_view()
                .map(|view| view.title.as_str()),
            Some(title_before_leaf.as_str())
        );

        app.handle(Action::ToggleArchitectureSourcePanel);
        assert!(!app.architecture_source_panel_enabled());
        assert_eq!(app.mode(), Mode::ArchitectureDiagram);
    }

    #[test]
    fn architecture_source_panel_reports_missing_project_root() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.start_default_view();
        app.handle(Action::ToggleArchitectureSourcePanel);
        assert!(app.architecture_source_panel_enabled());
        assert!(app.architecture_source_view().is_none());
        assert!(app
            .architecture_source_error()
            .is_some_and(|error| error.contains("project root")));
    }

    #[test]
    fn architecture_diagram_viewport_pan_center_and_overlays_are_bounded() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenArchitecture);
        app.handle(Action::OpenArchitectureDiagram);
        app.set_architecture_diagram_viewport(20, 7);

        app.handle(Action::PanRight);
        app.handle(Action::PanDown);
        let panned = app.architecture_diagram_pan();
        assert!(panned.0 > 0 || panned.1 > 0);
        assert!(app.status().contains("Architecture pan"));
        app.handle(Action::CenterDiagram);
        assert!(app.status().contains("centered"));

        app.handle(Action::OpenHelp);
        assert_eq!(app.mode(), Mode::Help);
        assert_eq!(app.overlay_return(), Mode::ArchitectureDiagram);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::ArchitectureDiagram);

        app.handle(Action::OpenDiagnostics);
        assert_eq!(app.mode(), Mode::Diagnostics);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::ArchitectureDiagram);

        app.set_architecture_diagram_viewport(8, 4);
        assert_eq!(app.mode(), Mode::ArchitectureDiagram);
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

    #[test]
    fn architecture_handler_covers_page_boundaries_and_overlays() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenArchitecture);
        app.handle(Action::End);
        assert_eq!(
            app.architecture_selected(),
            app.architecture_rows().len() - 1
        );
        app.handle(Action::PageUp);
        app.handle(Action::PageDown);
        app.handle(Action::Home);
        assert_eq!(app.architecture_selected(), 0);
        app.handle(Action::StartSearch);
        assert_eq!(app.mode(), Mode::Architecture);

        for (open, mode) in [
            (Action::OpenScope, Mode::ScopeDialog),
            (Action::OpenDiagnostics, Mode::Diagnostics),
            (Action::OpenHelp, Mode::Help),
        ] {
            app.handle(open);
            assert_eq!(app.mode(), mode);
            app.handle(Action::Cancel);
            assert_eq!(app.mode(), Mode::Architecture);
        }
    }

    #[test]
    fn diagram_handler_covers_all_pan_directions_and_page_actions() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenDiagram);
        app.set_diagram_viewport(24, 8);
        for action in [
            Action::PanUp,
            Action::PanDown,
            Action::PanLeft,
            Action::PanRight,
            Action::PageUp,
            Action::PageDown,
            Action::SelectNext,
            Action::SelectPrevious,
            Action::CenterDiagram,
        ] {
            app.handle(action);
        }
        app.handle(Action::StartSearch);
        assert_eq!(app.mode(), Mode::Diagram);
        assert!(app.status().contains("centered"));
    }

    #[test]
    fn scope_dialog_covers_boundaries_and_confirm_returns_to_diagram() {
        let codebase = fixture_codebase();
        let mut app = App::new(&codebase).expect("initial projection");
        app.handle(Action::OpenDiagram);
        app.handle(Action::OpenScope);
        app.handle(Action::End);
        app.handle(Action::PageUp);
        app.handle(Action::Home);
        app.handle(Action::PageDown);
        app.handle(Action::Confirm);
        assert_eq!(app.mode(), Mode::Diagram);
        app.handle(Action::StartSearch);
        assert_eq!(app.mode(), Mode::Diagram);

        app.handle(Action::OpenScope);
        app.handle(Action::Cancel);
        assert_eq!(app.mode(), Mode::Diagram);
    }
}
