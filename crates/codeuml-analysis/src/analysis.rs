use std::path::PathBuf;

use codeuml_core::{AnalysisMode, Codebase, Diagnostic};
use codeuml_lang::{LanguageProvider, ProjectPath, ProviderError, ProviderReport};
use thiserror::Error;

/// Analyze a project with every provider that recognises it.
///
/// Providers run in the order given and fill one shared [`Codebase`], so a
/// project mixing languages produces a single model. Recoverable per-file and
/// per-package problems are diagnostics. If several providers match and some
/// fail outright, the failure is recorded as an error diagnostic and the model
/// is marked partial; the run only fails when no provider succeeds.
pub fn analyze_project(
    project: &ProjectPath,
    providers: &[Box<dyn LanguageProvider>],
) -> Result<Codebase, AnalysisError> {
    let matching = detect_providers(project, providers);
    if matching.is_empty() {
        return Err(no_supported_project(project, providers));
    }

    let mut codebase = Codebase::new(project_name(project));
    let report = run_providers(project, &matching, &mut codebase)?;
    set_analysis_metadata(&mut codebase, report);
    Ok(codebase)
}

fn detect_providers<'a>(
    project: &ProjectPath,
    providers: &'a [Box<dyn LanguageProvider>],
) -> Vec<&'a dyn LanguageProvider> {
    providers
        .iter()
        .map(|provider| provider.as_ref())
        .filter(|provider| provider.detect(project))
        .collect()
}

fn no_supported_project(
    project: &ProjectPath,
    providers: &[Box<dyn LanguageProvider>],
) -> AnalysisError {
    let expected: Vec<&str> = providers
        .iter()
        .map(|provider| provider.detection_hint())
        .collect();
    AnalysisError::NoSupportedProject {
        root: project.as_path().to_path_buf(),
        expected: if expected.is_empty() {
            "no languages are enabled".to_owned()
        } else {
            expected.join(", ")
        },
    }
}

fn project_name(project: &ProjectPath) -> &str {
    project
        .as_path()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
}

fn run_providers(
    project: &ProjectPath,
    providers: &[&dyn LanguageProvider],
    codebase: &mut Codebase,
) -> Result<ProviderReport, AnalysisError> {
    let mut total = ProviderReport::default();
    let mut succeeded = 0;
    let mut failures = Vec::new();
    for provider in providers {
        match provider.analyze(project, codebase) {
            Ok(report) => {
                total = add_reports(total, report);
                succeeded += 1;
            }
            Err(error) => failures.push(error),
        }
    }
    record_failures(codebase, &mut total, succeeded, failures)?;
    Ok(total)
}

fn add_reports(total: ProviderReport, report: ProviderReport) -> ProviderReport {
    ProviderReport {
        files_analyzed: total.files_analyzed + report.files_analyzed,
        files_failed: total.files_failed + report.files_failed,
        partial: total.partial || report.partial,
    }
}

/// Fail only when nothing succeeded; otherwise keep the partial model and say
/// which provider could not contribute.
fn record_failures(
    codebase: &mut Codebase,
    total: &mut ProviderReport,
    succeeded: usize,
    failures: Vec<ProviderError>,
) -> Result<(), AnalysisError> {
    if succeeded == 0 {
        if let Some(first) = failures.into_iter().next() {
            return Err(first.into());
        }
        return Ok(());
    }
    for failure in failures {
        total.partial = true;
        codebase.add_diagnostic(
            Diagnostic::error(
                format!("{} analysis failed: {failure}", failure.provider()),
                None,
            )
            .with_code("analysis::provider-failed"),
        );
    }
    Ok(())
}

fn set_analysis_metadata(codebase: &mut Codebase, report: ProviderReport) {
    let metadata = codebase.analysis_mut();
    metadata.mode = AnalysisMode::SyntaxOnly;
    metadata.files_analyzed = report.files_analyzed;
    metadata.files_failed = report.files_failed;
    metadata.partial = report.partial || report.files_failed > 0;
}

/// Fatal analysis errors. Recoverable per-file and per-package issues are
/// returned as diagnostics in the [`Codebase`].
#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("no supported project found in `{}` (looked for: {expected})", root.display())]
    NoSupportedProject { root: PathBuf, expected: String },
}
