//! Python support for CodeUML.
//!
//! [`PythonProvider`] implements `codeuml_lang::LanguageProvider`. It finds the
//! project's `.py` files (skipping virtual environments, caches, build output
//! and test code), parses each with tree-sitter, and lowers classes, their
//! fields and methods, and the package/module tree into the language-neutral
//! `codeuml-core` model. Base classes and type annotations become
//! relationships, resolved syntactically through the file's imports.
//!
//! No Python interpreter is needed and no project code is executed.

mod annotation;
mod build;
mod discovery;
mod extract;
mod facts;
mod provider;
mod resolve;

#[cfg(test)]
mod test_support;

pub use provider::PythonProvider;
