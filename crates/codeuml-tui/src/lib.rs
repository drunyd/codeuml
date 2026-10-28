//! Terminal interface for CodeUML.
//!
//! The TUI consumes `codeuml-core` declarations and `codeuml-diagram`
//! projections. Application state, key handling, and rendering are separated so
//! navigation can be tested without a real terminal.

mod app;
mod format;
mod input;
mod render;
mod score;
mod source;
mod terminal;
mod terminal_diagram;

#[cfg(test)]
mod test_support;

pub use terminal::{run_browser, TuiError};
