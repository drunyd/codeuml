//! [`LanguageProvider`] for Python.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use codeuml_core::{Codebase, Diagnostic, Language, SourceFile};
use codeuml_lang::{LanguageProvider, ProjectPath, ProviderError, ProviderReport};

use crate::build::{add_modules, add_types, AnalyzedFile};
use crate::discovery::{discover, is_python_project, PySource};
use crate::extract::extract;
use crate::resolve::{Linker, Resolver};

const MAX_SOURCE_BYTES: u64 = 5 * 1024 * 1024;

/// Analyses Python projects with syntax-level analysis.
///
/// Classes (with their fields, methods, and enum members) and the modules and
/// packages containing them become the model; base classes and annotations
/// become relationships. Files that do not parse are reported as diagnostics
/// and skipped, so one broken file never hides the rest of the project.
#[derive(Clone, Copy, Debug, Default)]
pub struct PythonProvider;

impl LanguageProvider for PythonProvider {
    fn id(&self) -> &'static str {
        "python"
    }

    fn detection_hint(&self) -> &'static str {
        "Python (pyproject.toml, setup.py, requirements.txt, or .py files)"
    }

    fn detect(&self, root: &ProjectPath) -> bool {
        is_python_project(root.as_path())
    }

    fn analyze(
        &self,
        root: &ProjectPath,
        codebase: &mut Codebase,
    ) -> Result<ProviderReport, ProviderError> {
        let discovered = discover(root.as_path());
        let had_discovery_problems = !discovered.diagnostics.is_empty();
        for diagnostic in discovered.diagnostics {
            codebase.add_diagnostic(diagnostic);
        }
        let parsed = parse_sources(root.as_path(), &discovered.sources);
        for diagnostic in parsed.diagnostics {
            codebase.add_diagnostic(diagnostic);
        }
        let model_errors = build_model(codebase, &parsed.files);
        Ok(ProviderReport {
            files_analyzed: parsed.files.len(),
            files_failed: parsed.failed,
            partial: parsed.failed > 0 || had_discovery_problems || model_errors > 0,
        })
    }
}

struct Parsed {
    files: Vec<AnalyzedFile>,
    failed: usize,
    diagnostics: Vec<Diagnostic>,
}

fn parse_sources(root: &Path, sources: &[PySource]) -> Parsed {
    let mut parsed = Parsed {
        files: Vec::new(),
        failed: 0,
        diagnostics: Vec::new(),
    };
    for result in parse_in_parallel(root, sources) {
        match result {
            Ok(file) => parsed.files.push(file),
            Err(diagnostic) => {
                parsed.failed += 1;
                parsed.diagnostics.push(diagnostic);
            }
        }
    }
    parsed
}

type ParseResult = Result<AnalyzedFile, Diagnostic>;

/// Parsing dominates the run time and files are independent, so worker
/// threads repeatedly claim the next unparsed file (large files cannot pile up
/// on one thread). Results are put back in source order, so the outcome never
/// depends on scheduling.
fn parse_in_parallel(root: &Path, sources: &[PySource]) -> Vec<ParseResult> {
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(sources.len().max(1));
    let next = AtomicUsize::new(0);
    let mut parsed: Vec<(usize, ParseResult)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| scope.spawn(|| parse_claimed(root, sources, &next)))
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    parsed.sort_by_key(|(index, _)| *index);
    parsed.into_iter().map(|(_, result)| result).collect()
}

/// Parse files until none are left to claim.
fn parse_claimed(
    root: &Path,
    sources: &[PySource],
    next: &AtomicUsize,
) -> Vec<(usize, ParseResult)> {
    let mut done = Vec::new();
    loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let Some(source) = sources.get(index) else {
            return done;
        };
        done.push((index, parse_source(root, source)));
    }
}

fn failure(message: String, source: &PySource, code: &str) -> Diagnostic {
    Diagnostic::error(format!("{message} `{}`", source.path.display()), None).with_code(code)
}

fn parse_source(root: &Path, source: &PySource) -> Result<AnalyzedFile, Diagnostic> {
    let absolute = root.join(&source.path);
    let size = fs::metadata(&absolute).map(|metadata| metadata.len());
    if size.as_ref().is_ok_and(|size| *size > MAX_SOURCE_BYTES) {
        return Err(failure(
            "Python source is too large to analyze:".to_owned(),
            source,
            "python::source-too-large",
        ));
    }
    let text = fs::read_to_string(&absolute).map_err(|error| {
        failure(
            format!("could not read Python source ({error}):"),
            source,
            "python::unreadable-source",
        )
    })?;
    let facts = extract(&text, &source.path).map_err(|error| {
        Diagnostic::error(
            format!("failed to parse Python source: {}", error.message),
            Some(error.location),
        )
        .with_code("python::parse-error")
    })?;
    Ok(AnalyzedFile {
        module: source.module_name(),
        source: source.clone(),
        facts,
    })
}

/// Add files, modules, types and relationships; returns how many model
/// invariant violations were reported as diagnostics.
fn build_model(codebase: &mut Codebase, files: &[AnalyzedFile]) -> usize {
    for file in files {
        codebase.add_file(SourceFile::new(&file.source.path, Language::Python));
    }
    let sources: Vec<PySource> = files.iter().map(|file| file.source.clone()).collect();
    let mut errors = Vec::new();
    let modules = add_modules(codebase, &sources, &mut errors);
    let declared = add_types(codebase, files, &modules, &mut errors);
    let module_names: BTreeSet<String> = modules.keys().cloned().collect();
    let resolver = Resolver::new(files, &declared, &module_names);
    let mut linker = Linker {
        resolver: &resolver,
        codebase,
        diagnostics: Vec::new(),
    };
    for entry in &declared {
        linker.link_class(files, entry);
    }
    let diagnostics = linker.diagnostics;
    let invariant_errors = errors.len();
    for message in errors {
        codebase.add_diagnostic(
            Diagnostic::error(message, None).with_code("analysis::model-invariant"),
        );
    }
    for diagnostic in diagnostics {
        codebase.add_diagnostic(diagnostic);
    }
    invariant_errors
}
