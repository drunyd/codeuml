use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{Action, Mode};

pub(crate) fn action_for_key(mode: Mode, key: KeyEvent) -> Option<Action> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    if control && matches!(key.code, KeyCode::Char('c')) {
        return Some(Action::Quit);
    }

    match mode {
        Mode::Search => match key.code {
            KeyCode::Esc => Some(Action::SearchCancel),
            KeyCode::Enter => Some(Action::SearchSubmit),
            KeyCode::Backspace => Some(Action::SearchBackspace),
            KeyCode::Char(character) if !control => Some(Action::SearchInput(character)),
            _ => None,
        },
        Mode::Browse => match key.code {
            KeyCode::Char('u') if control => Some(Action::ScrollDetailsUp),
            KeyCode::Char('d') if control => Some(Action::ScrollDetailsDown),
            KeyCode::Up | KeyCode::Char('k') if !control => Some(Action::NavigateUp),
            KeyCode::Down | KeyCode::Char('j') if !control => Some(Action::NavigateDown),
            KeyCode::PageUp => Some(Action::PageUp),
            KeyCode::PageDown => Some(Action::PageDown),
            KeyCode::Home => Some(Action::Home),
            KeyCode::End => Some(Action::End),
            KeyCode::Enter => Some(Action::Confirm),
            KeyCode::Tab => Some(Action::ToggleFocus),
            KeyCode::Esc => Some(Action::Cancel),
            KeyCode::Char('/') if !control => Some(Action::StartSearch),
            KeyCode::Char('s') if !control => Some(Action::OpenScope),
            KeyCode::Char('d') if !control => Some(Action::OpenDiagnostics),
            KeyCode::Char('?') if !control => Some(Action::OpenHelp),
            KeyCode::Char('v') if !control => Some(Action::OpenDiagram),
            KeyCode::Char('q') if !control => Some(Action::Quit),
            _ => None,
        },
        Mode::Diagram => match key.code {
            KeyCode::Up | KeyCode::Char('k') if !control => Some(Action::PanUp),
            KeyCode::Down | KeyCode::Char('j') if !control => Some(Action::PanDown),
            KeyCode::Left | KeyCode::Char('h') if !control => Some(Action::PanLeft),
            KeyCode::Right | KeyCode::Char('l') if !control => Some(Action::PanRight),
            KeyCode::PageUp => Some(Action::PageUp),
            KeyCode::PageDown => Some(Action::PageDown),
            KeyCode::Tab | KeyCode::Char('n') if !control => Some(Action::SelectNext),
            KeyCode::BackTab | KeyCode::Char('p') if !control => Some(Action::SelectPrevious),
            KeyCode::Char('c') if !control => Some(Action::CenterDiagram),
            KeyCode::Enter => Some(Action::Confirm),
            KeyCode::Esc | KeyCode::Char('b') | KeyCode::Char('v') => Some(Action::Cancel),
            KeyCode::Char('s') if !control => Some(Action::OpenScope),
            KeyCode::Char('d') if !control => Some(Action::OpenDiagnostics),
            KeyCode::Char('?') if !control => Some(Action::OpenHelp),
            KeyCode::Char('q') if !control => Some(Action::Quit),
            _ => None,
        },
        Mode::Diagnostics => match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(Action::NavigateUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::NavigateDown),
            KeyCode::PageUp => Some(Action::PageUp),
            KeyCode::PageDown => Some(Action::PageDown),
            KeyCode::Home => Some(Action::Home),
            KeyCode::End => Some(Action::End),
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::Cancel),
            _ => None,
        },
        Mode::ScopeDialog => match key.code {
            KeyCode::Up | KeyCode::Char('k') => Some(Action::NavigateUp),
            KeyCode::Down | KeyCode::Char('j') => Some(Action::NavigateDown),
            KeyCode::PageUp => Some(Action::PageUp),
            KeyCode::PageDown => Some(Action::PageDown),
            KeyCode::Home => Some(Action::Home),
            KeyCode::End => Some(Action::End),
            KeyCode::Enter => Some(Action::Confirm),
            KeyCode::Esc | KeyCode::Char('q') => Some(Action::Cancel),
            _ => None,
        },
        Mode::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => Some(Action::Cancel),
            _ => None,
        },
    }
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
