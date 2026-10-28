use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{Action, Mode};

type Binding = (KeyCode, Action, bool);

const fn binding(code: KeyCode, action: Action, allow_control: bool) -> Binding {
    (code, action, allow_control)
}

const BROWSE: &[Binding] = &[
    binding(KeyCode::Up, Action::NavigateUp, false),
    binding(KeyCode::Char('k'), Action::NavigateUp, false),
    binding(KeyCode::Down, Action::NavigateDown, false),
    binding(KeyCode::Char('j'), Action::NavigateDown, false),
    binding(KeyCode::PageUp, Action::PageUp, true),
    binding(KeyCode::PageDown, Action::PageDown, true),
    binding(KeyCode::Home, Action::Home, true),
    binding(KeyCode::End, Action::End, true),
    binding(KeyCode::Enter, Action::Confirm, true),
    binding(KeyCode::Tab, Action::ToggleFocus, true),
    binding(KeyCode::Esc, Action::Cancel, true),
    binding(KeyCode::Char('/'), Action::StartSearch, false),
    binding(KeyCode::Char('s'), Action::OpenScope, false),
    binding(KeyCode::Char('d'), Action::OpenDiagnostics, false),
    binding(KeyCode::Char('?'), Action::OpenHelp, false),
    binding(KeyCode::Char('v'), Action::OpenDiagram, false),
    binding(KeyCode::Char('a'), Action::OpenArchitecture, false),
    binding(KeyCode::Char('o'), Action::OpenSource, false),
    binding(KeyCode::Backspace, Action::Back, true),
    binding(KeyCode::Char('q'), Action::Quit, false),
];
const BROWSE_CONTROL: &[(KeyCode, Action)] = &[
    (KeyCode::Char('u'), Action::ScrollDetailsUp),
    (KeyCode::Char('d'), Action::ScrollDetailsDown),
];
const SOURCE: &[Binding] = &[
    binding(KeyCode::Up, Action::NavigateUp, false),
    binding(KeyCode::Char('k'), Action::NavigateUp, false),
    binding(KeyCode::Down, Action::NavigateDown, false),
    binding(KeyCode::Char('j'), Action::NavigateDown, false),
    binding(KeyCode::PageUp, Action::PageUp, true),
    binding(KeyCode::PageDown, Action::PageDown, true),
    binding(KeyCode::Home, Action::Home, true),
    binding(KeyCode::End, Action::End, true),
    binding(KeyCode::Esc, Action::Cancel, true),
    binding(KeyCode::Char('b'), Action::Cancel, true),
    binding(KeyCode::Char('o'), Action::Cancel, true),
    binding(KeyCode::Char('q'), Action::Cancel, true),
];
const ARCHITECTURE: &[Binding] = &[
    binding(KeyCode::Char('o'), Action::OpenSource, false),
    binding(KeyCode::Char('v'), Action::OpenArchitectureDiagram, false),
    binding(KeyCode::Char('t'), Action::OpenTypeBrowser, false),
    binding(KeyCode::Up, Action::NavigateUp, false),
    binding(KeyCode::Char('k'), Action::NavigateUp, false),
    binding(KeyCode::Down, Action::NavigateDown, false),
    binding(KeyCode::Char('j'), Action::NavigateDown, false),
    binding(KeyCode::PageUp, Action::PageUp, true),
    binding(KeyCode::PageDown, Action::PageDown, true),
    binding(KeyCode::Home, Action::Home, true),
    binding(KeyCode::End, Action::End, true),
    binding(KeyCode::Enter, Action::Confirm, true),
    binding(KeyCode::Right, Action::Descend, false),
    binding(KeyCode::Char('l'), Action::Descend, false),
    binding(KeyCode::Left, Action::Ascend, false),
    binding(KeyCode::Char('h'), Action::Ascend, false),
    binding(KeyCode::Esc, Action::Cancel, false),
    binding(KeyCode::Char('b'), Action::Cancel, false),
    binding(KeyCode::Char('s'), Action::OpenScope, false),
    binding(KeyCode::Char('d'), Action::OpenDiagnostics, false),
    binding(KeyCode::Char('?'), Action::OpenHelp, false),
    binding(KeyCode::Char('q'), Action::Quit, false),
];
const ARCHITECTURE_DIAGRAM: &[Binding] = &[
    binding(KeyCode::Up, Action::NavigateUp, false),
    binding(KeyCode::Char('k'), Action::NavigateUp, false),
    binding(KeyCode::Down, Action::NavigateDown, false),
    binding(KeyCode::Char('j'), Action::NavigateDown, false),
    binding(KeyCode::Home, Action::Home, true),
    binding(KeyCode::End, Action::End, true),
    binding(KeyCode::PageUp, Action::PageUp, true),
    binding(KeyCode::PageDown, Action::PageDown, true),
    binding(KeyCode::Enter, Action::Confirm, true),
    binding(KeyCode::Right, Action::Descend, false),
    binding(KeyCode::Char('l'), Action::Descend, false),
    binding(KeyCode::Left, Action::Ascend, false),
    binding(KeyCode::Char('h'), Action::Ascend, false),
    binding(KeyCode::Char('f'), Action::ToggleArchitectureFull, false),
    binding(KeyCode::Char('t'), Action::OpenTypeBrowser, false),
    binding(KeyCode::Char('c'), Action::CenterDiagram, false),
    binding(KeyCode::Char('o'), Action::OpenSource, false),
    binding(
        KeyCode::Char('s'),
        Action::ToggleArchitectureSourcePanel,
        false,
    ),
    binding(KeyCode::Esc, Action::Cancel, true),
    binding(KeyCode::Char('b'), Action::Cancel, true),
    binding(KeyCode::Char('v'), Action::Cancel, true),
    binding(KeyCode::Char('d'), Action::OpenDiagnostics, false),
    binding(KeyCode::Char('?'), Action::OpenHelp, false),
    binding(KeyCode::Char('q'), Action::Quit, false),
];
const ARCHITECTURE_DIAGRAM_CONTROL: &[(KeyCode, Action)] = &[
    (KeyCode::Char('h'), Action::PanLeft),
    (KeyCode::Char('j'), Action::PanDown),
    (KeyCode::Char('k'), Action::PanUp),
    (KeyCode::Char('l'), Action::PanRight),
    (KeyCode::Char('u'), Action::ScrollDetailsUp),
    (KeyCode::Char('d'), Action::ScrollDetailsDown),
];
const DIAGRAM: &[Binding] = &[
    binding(KeyCode::Up, Action::PanUp, false),
    binding(KeyCode::Char('k'), Action::PanUp, false),
    binding(KeyCode::Down, Action::PanDown, false),
    binding(KeyCode::Char('j'), Action::PanDown, false),
    binding(KeyCode::Left, Action::PanLeft, false),
    binding(KeyCode::Char('h'), Action::PanLeft, false),
    binding(KeyCode::Right, Action::PanRight, false),
    binding(KeyCode::Char('l'), Action::PanRight, false),
    binding(KeyCode::PageUp, Action::PageUp, true),
    binding(KeyCode::PageDown, Action::PageDown, true),
    binding(KeyCode::Tab, Action::SelectNext, false),
    binding(KeyCode::Char('n'), Action::SelectNext, false),
    binding(KeyCode::BackTab, Action::SelectPrevious, false),
    binding(KeyCode::Char('p'), Action::SelectPrevious, false),
    binding(KeyCode::Char('c'), Action::CenterDiagram, false),
    binding(KeyCode::Char('o'), Action::OpenSource, false),
    binding(KeyCode::Enter, Action::Confirm, true),
    binding(KeyCode::Esc, Action::Cancel, true),
    binding(KeyCode::Char('b'), Action::Cancel, true),
    binding(KeyCode::Char('v'), Action::Cancel, true),
    binding(KeyCode::Char('s'), Action::OpenScope, false),
    binding(KeyCode::Char('d'), Action::OpenDiagnostics, false),
    binding(KeyCode::Char('?'), Action::OpenHelp, false),
    binding(KeyCode::Char('q'), Action::Quit, false),
];
const DIAGNOSTICS: &[Binding] = &[
    binding(KeyCode::Up, Action::NavigateUp, true),
    binding(KeyCode::Char('k'), Action::NavigateUp, true),
    binding(KeyCode::Down, Action::NavigateDown, true),
    binding(KeyCode::Char('j'), Action::NavigateDown, true),
    binding(KeyCode::PageUp, Action::PageUp, true),
    binding(KeyCode::PageDown, Action::PageDown, true),
    binding(KeyCode::Home, Action::Home, true),
    binding(KeyCode::End, Action::End, true),
    binding(KeyCode::Esc, Action::Cancel, true),
    binding(KeyCode::Char('q'), Action::Cancel, true),
];
const SCOPE_DIALOG: &[Binding] = &[
    binding(KeyCode::Up, Action::NavigateUp, true),
    binding(KeyCode::Char('k'), Action::NavigateUp, true),
    binding(KeyCode::Down, Action::NavigateDown, true),
    binding(KeyCode::Char('j'), Action::NavigateDown, true),
    binding(KeyCode::PageUp, Action::PageUp, true),
    binding(KeyCode::PageDown, Action::PageDown, true),
    binding(KeyCode::Home, Action::Home, true),
    binding(KeyCode::End, Action::End, true),
    binding(KeyCode::Enter, Action::Confirm, true),
    binding(KeyCode::Esc, Action::Cancel, true),
    binding(KeyCode::Char('q'), Action::Cancel, true),
];
const HELP: &[Binding] = &[
    binding(KeyCode::Esc, Action::Cancel, true),
    binding(KeyCode::Char('q'), Action::Cancel, true),
    binding(KeyCode::Char('?'), Action::Cancel, true),
];

fn find_binding(code: KeyCode, control: bool, bindings: &[Binding]) -> Option<Action> {
    bindings
        .iter()
        .find(|(candidate, _, allow_control)| *candidate == code && (*allow_control || !control))
        .map(|(_, action, _)| action.clone())
}

fn action_for_search(key: KeyEvent, control: bool) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::SearchCancel),
        KeyCode::Enter => Some(Action::SearchSubmit),
        KeyCode::Backspace => Some(Action::SearchBackspace),
        KeyCode::Char(character) if !control => Some(Action::SearchInput(character)),
        _ => None,
    }
}

pub(crate) fn action_for_key(mode: Mode, key: KeyEvent) -> Option<Action> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if let Some(action) = control_action(mode, key.code, control) {
        return Some(action);
    }
    if mode == Mode::Search {
        return action_for_search(key, control);
    }
    action_for_mode(mode, key.code, control)
}

fn action_for_mode(mode: Mode, code: KeyCode, control: bool) -> Option<Action> {
    match mode {
        Mode::Search => None,
        Mode::Browse => find_binding(code, control, BROWSE),
        Mode::Source => find_binding(code, control, SOURCE),
        Mode::Architecture => find_binding(code, control, ARCHITECTURE),
        Mode::ArchitectureDiagram => find_binding(code, control, ARCHITECTURE_DIAGRAM),
        Mode::Diagram => find_binding(code, control, DIAGRAM),
        Mode::Diagnostics => find_binding(code, control, DIAGNOSTICS),
        Mode::ScopeDialog => find_binding(code, control, SCOPE_DIALOG),
        Mode::Help => find_binding(code, control, HELP),
    }
}

fn control_action(mode: Mode, code: KeyCode, control: bool) -> Option<Action> {
    if is_control_c(code, control) {
        return Some(Action::Quit);
    }
    if mode == Mode::Browse && control {
        return find_control_binding(code, BROWSE_CONTROL);
    }
    if mode == Mode::ArchitectureDiagram && control {
        return find_control_binding(code, ARCHITECTURE_DIAGRAM_CONTROL);
    }
    None
}

fn is_control_c(code: KeyCode, control: bool) -> bool {
    control && code == KeyCode::Char('c')
}

fn find_control_binding(code: KeyCode, bindings: &[(KeyCode, Action)]) -> Option<Action> {
    bindings
        .iter()
        .find(|(candidate, _)| *candidate == code)
        .map(|(_, action)| action.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn control(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
    }

    #[test]
    fn browse_keys_cover_navigation_and_commands() {
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Up)),
            Some(Action::NavigateUp)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('j'))),
            Some(Action::NavigateDown)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::PageDown)),
            Some(Action::PageDown)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Home)),
            Some(Action::Home)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Tab)),
            Some(Action::ToggleFocus)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('/'))),
            Some(Action::StartSearch)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('s'))),
            Some(Action::OpenScope)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('d'))),
            Some(Action::OpenDiagnostics)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('?'))),
            Some(Action::OpenHelp)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('q'))),
            Some(Action::Quit)
        );
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('a'))),
            Some(Action::OpenArchitecture)
        );
        assert_eq!(
            action_for_key(Mode::Browse, control('u')),
            Some(Action::ScrollDetailsUp)
        );
        assert_eq!(
            action_for_key(Mode::Browse, control('d')),
            Some(Action::ScrollDetailsDown)
        );
    }

    #[test]
    fn ctrl_c_always_quits_but_q_is_text_during_search() {
        for mode in [
            Mode::Browse,
            Mode::Architecture,
            Mode::ArchitectureDiagram,
            Mode::Source,
            Mode::Diagram,
            Mode::Search,
            Mode::Diagnostics,
            Mode::ScopeDialog,
            Mode::Help,
        ] {
            assert_eq!(
                action_for_key(mode, control('c')),
                Some(Action::Quit),
                "mode {mode:?}"
            );
        }
        assert_eq!(
            action_for_key(Mode::Search, key(KeyCode::Char('q'))),
            Some(Action::SearchInput('q'))
        );
    }

    #[test]
    fn search_keys_handle_typing_backspace_submit_and_cancel() {
        assert_eq!(
            action_for_key(Mode::Search, key(KeyCode::Char('A'))),
            Some(Action::SearchInput('A'))
        );
        assert_eq!(
            action_for_key(Mode::Search, key(KeyCode::Backspace)),
            Some(Action::SearchBackspace)
        );
        assert_eq!(
            action_for_key(Mode::Search, key(KeyCode::Enter)),
            Some(Action::SearchSubmit)
        );
        assert_eq!(
            action_for_key(Mode::Search, key(KeyCode::Esc)),
            Some(Action::SearchCancel)
        );
    }

    #[test]
    fn diagram_keys_cover_pan_focus_center_open_and_return() {
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Char('v'))),
            Some(Action::OpenDiagram)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Up)),
            Some(Action::PanUp)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Char('h'))),
            Some(Action::PanLeft)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Tab)),
            Some(Action::SelectNext)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::BackTab)),
            Some(Action::SelectPrevious)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Char('c'))),
            Some(Action::CenterDiagram)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Enter)),
            Some(Action::Confirm)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Esc)),
            Some(Action::Cancel)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Char('b'))),
            Some(Action::Cancel)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::PageDown)),
            Some(Action::PageDown)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Char('s'))),
            Some(Action::OpenScope)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Char('d'))),
            Some(Action::OpenDiagnostics)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Char('?'))),
            Some(Action::OpenHelp)
        );
        assert_eq!(
            action_for_key(Mode::Diagram, key(KeyCode::Char('q'))),
            Some(Action::Quit)
        );
    }

    #[test]
    fn architecture_keys_navigate_drill_and_return() {
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Down)),
            Some(Action::NavigateDown)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Enter)),
            Some(Action::Confirm)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Char('l'))),
            Some(Action::Descend)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Right)),
            Some(Action::Descend)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Char('h'))),
            Some(Action::Ascend)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Left)),
            Some(Action::Ascend)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Esc)),
            Some(Action::Cancel)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Char('b'))),
            Some(Action::Cancel)
        );
    }

    #[test]
    fn architecture_diagram_keys_select_zoom_full_and_pan() {
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Char('v'))),
            Some(Action::OpenArchitectureDiagram)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::Char('j'))),
            Some(Action::NavigateDown)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::Char('l'))),
            Some(Action::Descend)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::Char('h'))),
            Some(Action::Ascend)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::Char('f'))),
            Some(Action::ToggleArchitectureFull)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::Char('t'))),
            Some(Action::OpenTypeBrowser)
        );
        assert_eq!(
            action_for_key(Mode::Architecture, key(KeyCode::Char('t'))),
            Some(Action::OpenTypeBrowser)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::Char('v'))),
            Some(Action::Cancel)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::Char('s'))),
            Some(Action::ToggleArchitectureSourcePanel)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, control('u')),
            Some(Action::ScrollDetailsUp)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, control('l')),
            Some(Action::PanRight)
        );
        assert_eq!(
            action_for_key(Mode::ArchitectureDiagram, key(KeyCode::PageDown)),
            Some(Action::PageDown)
        );
    }

    #[test]
    fn source_and_back_keys_are_available_in_the_right_modes() {
        for mode in [
            Mode::Browse,
            Mode::Architecture,
            Mode::ArchitectureDiagram,
            Mode::Diagram,
        ] {
            assert_eq!(
                action_for_key(mode, key(KeyCode::Char('o'))),
                Some(Action::OpenSource),
                "mode {mode:?}"
            );
        }
        assert_eq!(
            action_for_key(Mode::Browse, key(KeyCode::Backspace)),
            Some(Action::Back)
        );
        assert_eq!(
            action_for_key(Mode::Source, key(KeyCode::Char('j'))),
            Some(Action::NavigateDown)
        );
        for code in [KeyCode::Esc, KeyCode::Char('o'), KeyCode::Char('q')] {
            assert_eq!(
                action_for_key(Mode::Source, key(code)),
                Some(Action::Cancel)
            );
        }
    }

    #[test]
    fn diagnostics_scope_and_help_modes_close_without_quitting() {
        for mode in [Mode::Diagnostics, Mode::ScopeDialog, Mode::Help] {
            assert_eq!(
                action_for_key(mode, key(KeyCode::Esc)),
                Some(Action::Cancel),
                "mode {mode:?}"
            );
            assert_eq!(
                action_for_key(mode, key(KeyCode::Char('q'))),
                Some(Action::Cancel),
                "mode {mode:?}"
            );
        }
        assert_eq!(
            action_for_key(Mode::ScopeDialog, key(KeyCode::Enter)),
            Some(Action::Confirm)
        );
    }
}
