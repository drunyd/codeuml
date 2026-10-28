use codeuml_core::Codebase;
use codeuml_diagram::ProjectionError;
use codeuml_metrics::Metrics;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    crossterm::{
        event::{self, Event, KeyEventKind},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    Terminal,
};
use std::io;
use std::path::Path;
use thiserror::Error;

use crate::{app::App, input::action_for_key, render::render};

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("terminal I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("could not construct the initial diagram: {0}")]
    Projection(#[from] ProjectionError),
}

/// Run the interactive browser until the user quits. `project_root` is used to
/// show source excerpts for declarations and relationship evidence; `metrics`,
/// when present, adds colour-coded CRAP scores next to modules, types and methods.
pub fn run_browser(
    codebase: &Codebase,
    project_root: &Path,
    metrics: Option<Metrics>,
) -> Result<(), TuiError> {
    let mut app = App::new(codebase)?
        .with_project_root(project_root.to_path_buf())
        .with_metrics(metrics);
    app.start_default_view();
    run_browser_app(app)
}

fn run_browser_app(mut app: App<'_>) -> Result<(), TuiError> {
    let guard = TerminalGuard::enter()?;
    let result = Terminal::new(CrosstermBackend::new(io::stdout()))
        .map_err(TuiError::from)
        .and_then(|mut terminal| run_event_loop(&mut terminal, &mut app, event::read));
    drop(guard);
    result
}

fn run_event_loop<B, E>(
    terminal: &mut Terminal<B>,
    app: &mut App<'_>,
    mut next_event: E,
) -> Result<(), TuiError>
where
    B: Backend,
    E: FnMut() -> io::Result<Event>,
{
    loop {
        terminal.draw(|frame| render(frame, app))?;
        if app.should_quit() {
            return Ok(());
        }

        let event = next_event()?;
        handle_event(app, event);

        if app.should_quit() {
            return Ok(());
        }
    }
}

fn handle_event(app: &mut App<'_>, event: Event) {
    if let Event::Key(key) = event {
        if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            if let Some(action) = action_for_key(app.mode(), key) {
                app.handle(action);
            }
        }
    }
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        Self::enter_alternate_screen()
    }

    /// Leaves raw mode again when the alternate screen cannot be entered.
    fn enter_alternate_screen() -> io::Result<Self> {
        let entered = execute!(io::stdout(), EnterAlternateScreen);
        if entered.is_err() {
            let _ = disable_raw_mode();
        }
        entered.map(|()| Self { active: true })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_if_active(self.active, restore_terminal);
    }
}

fn restore_if_active(active: bool, restore: impl FnOnce()) {
    if active {
        restore();
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        LeaveAlternateScreen,
        ratatui::crossterm::cursor::Show
    );
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, collections::VecDeque};

    use ratatui::{
        backend::TestBackend,
        crossterm::event::{KeyCode, KeyEvent, KeyModifiers},
    };

    use super::*;

    #[test]
    fn cleanup_runs_only_for_an_active_terminal_guard() {
        let cleaned = Cell::new(false);
        restore_if_active(true, || cleaned.set(true));
        assert!(cleaned.get());

        let cleaned = Cell::new(false);
        restore_if_active(false, || cleaned.set(true));
        assert!(!cleaned.get());
    }

    #[test]
    fn terminal_errors_preserve_their_context() {
        let error = TuiError::Io(io::Error::other("render failed"));
        assert!(error.to_string().contains("render failed"));
    }

    #[test]
    fn scripted_event_loop_handles_resize_release_and_quit() {
        let codebase = crate::test_support::fixture_codebase();
        let mut app = App::new(&codebase).expect("initial app");
        let events = VecDeque::from([
            Event::Resize(80, 24),
            Event::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            )),
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ]);
        let mut events = events.into_iter();
        let consumed = Cell::new(0);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

        run_event_loop(&mut terminal, &mut app, || {
            consumed.set(consumed.get() + 1);
            Ok(events.next().expect("scripted event"))
        })
        .expect("event loop");

        assert!(app.should_quit());
        assert_eq!(consumed.get(), 4, "release and non-key events are ignored");
    }

    #[test]
    fn scripted_event_loop_propagates_event_errors() {
        let codebase = crate::test_support::fixture_codebase();
        let mut app = App::new(&codebase).expect("initial app");
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

        let error = run_event_loop(&mut terminal, &mut app, || {
            Err(io::Error::other("event source failed"))
        })
        .expect_err("event source error");

        assert!(error.to_string().contains("event source failed"));
    }

    #[test]
    fn already_quitting_app_does_not_read_an_event() {
        let codebase = crate::test_support::fixture_codebase();
        let mut app = App::new(&codebase).expect("initial app");
        app.handle(crate::app::Action::Quit);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

        run_event_loop(&mut terminal, &mut app, || {
            panic!("quit app must not request another event")
        })
        .expect("event loop exits");
    }
}
