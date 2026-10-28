use std::io;

use codeuml_core::Codebase;
use codeuml_diagram::ProjectionError;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyEventKind},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    Terminal,
};
use thiserror::Error;

use crate::{app::App, input::action_for_key, render::render};

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("terminal I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("could not construct the initial diagram: {0}")]
    Projection(#[from] ProjectionError),
}

/// Run the interactive browser until the user quits.
pub fn run_browser(codebase: &Codebase) -> Result<(), TuiError> {
    let mut app = App::new(codebase)?;
    let guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let result = run_event_loop(&mut terminal, &mut app);
    drop(terminal);
    drop(guard);
    result
}

fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App<'_>,
) -> Result<(), TuiError> {
    loop {
        terminal.draw(|frame| render(frame, app))?;
        if app.should_quit() {
            return Ok(());
        }

        let event = event::read()?;
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if let Some(action) = action_for_key(app.mode(), key) {
                    app.handle(action);
                }
            }
            Event::Resize(_, _) => {}
            _ => {}
        }

        if app.should_quit() {
            return Ok(());
        }
    }
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(Self { active: true })
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
    use std::cell::Cell;

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
}
