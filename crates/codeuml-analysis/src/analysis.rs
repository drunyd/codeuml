use codeuml_core::{AnalysisMode, Codebase, Diagnostic, Language, SourceFile};
use codeuml_lang_rust::{
    analyze_package, RustAnalysis, RustPackageSources, RustReference, RustTargetSource,
};
use thiserror::Error;

use crate::resolution::resolve_references;
use crate::{discover_project, DiscoveredPackage, DiscoveryError, ProjectPath};

/// Analyze a Cargo project with syntax-level Rust analysis.
///
/// Discovery and each package analyzer run independently; per-file parse
/// failures and recoverable discovery problems become diagnostics rather than
/// discarding the rest of the model.
pub fn analyze_project(project: &ProjectPath) -> Result<Codebase, AnalysisError> {
    let discovered = discover_project(project)?;
    let project_name = project
        .as_path()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let mut codebase = Codebase::new(project_name);

    add_discovered_files(&mut codebase, &discovered);
    let had_discovery_problems = add_discovery_diagnostics(&mut codebase, &discovered);
    let package_analyses = analyze_packages(&discovered);
    let (files_analyzed, files_failed, model_errors) =
        merge_package_analyses(&mut codebase, &package_analyses);
    let references = collect_references(&package_analyses);
    resolve_references(&mut codebase, &references);
    add_model_error_diagnostics(&mut codebase, &model_errors);
    set_analysis_metadata(
        &mut codebase,
        files_analyzed,
        files_failed,
        had_discovery_problems,
        &model_errors,
    );
    Ok(codebase)
}

fn add_discovered_files(codebase: &mut Codebase, discovered: &crate::DiscoveredProject) {
    for file in discovered.source_files() {
        codebase.add_file(SourceFile::new(file, Language::Rust));
    }
}

fn add_discovery_diagnostics(
    codebase: &mut Codebase,
    discovered: &crate::DiscoveredProject,
) -> bool {
    let diagnostics = discovered.diagnostics();
    for diagnostic in diagnostics {
        codebase.add_diagnostic(diagnostic.clone());
    }
    !diagnostics.is_empty()
}

fn analyze_packages(discovered: &crate::DiscoveredProject) -> Vec<RustAnalysis> {
    discovered
        .packages()
        .iter()
        .map(rust_package_sources)
        .map(|package| analyze_package(discovered.root(), &package))
        .collect()
}

fn merge_package_analyses(
    codebase: &mut Codebase,
    analyses: &[RustAnalysis],
) -> (usize, usize, Vec<String>) {
    let mut files_analyzed = 0;
    let mut files_failed = 0;
    let mut model_errors = Vec::new();
    for analysis in analyses {
        files_analyzed += analysis.files_analyzed;
        files_failed += analysis.files_failed;
        add_package_diagnostics(codebase, analysis);
        add_package_modules(codebase, analysis, &mut model_errors);
        add_package_types(codebase, analysis, &mut model_errors);
    }
    (files_analyzed, files_failed, model_errors)
}

fn add_package_diagnostics(codebase: &mut Codebase, analysis: &RustAnalysis) {
    for diagnostic in &analysis.diagnostics {
        codebase.add_diagnostic(diagnostic.clone());
    }
}

fn add_package_modules(
    codebase: &mut Codebase,
    analysis: &RustAnalysis,
    model_errors: &mut Vec<String>,
) {
    for module in &analysis.modules {
        if let Err(error) = codebase.add_module(module.clone()) {
            model_errors.push(format!("could not add module: {error}"));
        }
    }
}

fn add_package_types(
    codebase: &mut Codebase,
    analysis: &RustAnalysis,
    model_errors: &mut Vec<String>,
) {
    for declaration in &analysis.types {
        if let Err(error) = codebase.add_type(declaration.clone()) {
            model_errors.push(format!("could not add type declaration: {error}"));
        }
    }
}

fn collect_references(analyses: &[RustAnalysis]) -> Vec<RustReference> {
    analyses
        .iter()
        .flat_map(|analysis| analysis.references.iter().cloned())
        .collect()
}

fn add_model_error_diagnostics(codebase: &mut Codebase, model_errors: &[String]) {
    for message in model_errors {
        codebase.add_diagnostic(
            Diagnostic::error(message.clone(), None).with_code("analysis::model-invariant"),
        );
    }
}

fn set_analysis_metadata(
    codebase: &mut Codebase,
    files_analyzed: usize,
    files_failed: usize,
    had_discovery_problems: bool,
    model_errors: &[String],
) {
    let metadata = codebase.analysis_mut();
    metadata.mode = AnalysisMode::SyntaxOnly;
    metadata.files_analyzed = files_analyzed;
    metadata.files_failed = files_failed;
    metadata.partial = files_failed > 0 || had_discovery_problems || !model_errors.is_empty();
}

fn rust_package_sources(package: &DiscoveredPackage) -> RustPackageSources {
    RustPackageSources {
        name: package.name.clone(),
        source_files: package.source_files.clone(),
        targets: package
            .targets
            .iter()
            .map(|target| RustTargetSource {
                name: target.name.clone(),
                source_path: target.source_path.clone(),
                is_library: is_library_target(&target.kinds),
            })
            .collect(),
    }
}

fn is_library_target(kinds: &[String]) -> bool {
    kinds.iter().any(|kind| {
        matches!(
            kind.as_str(),
            "lib" | "proc-macro" | "rlib" | "dylib" | "cdylib" | "staticlib"
        )
    })
}

/// Fatal analysis errors. Recoverable per-file and per-package issues are
/// returned as diagnostics in the [`Codebase`].
#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
}
