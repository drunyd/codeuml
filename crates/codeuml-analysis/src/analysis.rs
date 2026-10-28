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

    for file in discovered.source_files() {
        codebase.add_file(SourceFile::new(file, Language::Rust));
    }

    let discovery_diagnostics = discovered.diagnostics().to_vec();
    let had_discovery_problems = !discovery_diagnostics.is_empty();
    for diagnostic in discovery_diagnostics {
        codebase.add_diagnostic(diagnostic);
    }

    let packages: Vec<RustPackageSources> = discovered
        .packages()
        .iter()
        .map(rust_package_sources)
        .collect();
    let package_analyses: Vec<RustAnalysis> = packages
        .iter()
        .map(|package| analyze_package(discovered.root(), package))
        .collect();

    let mut model_errors = Vec::new();
    let mut files_analyzed = 0;
    let mut files_failed = 0;

    for analysis in &package_analyses {
        files_analyzed += analysis.files_analyzed;
        files_failed += analysis.files_failed;
        for diagnostic in &analysis.diagnostics {
            codebase.add_diagnostic(diagnostic.clone());
        }

        for module in &analysis.modules {
            if let Err(error) = codebase.add_module(module.clone()) {
                model_errors.push(format!("could not add module: {error}"));
            }
        }
        for declaration in &analysis.types {
            if let Err(error) = codebase.add_type(declaration.clone()) {
                model_errors.push(format!("could not add type declaration: {error}"));
            }
        }
    }

    let references: Vec<RustReference> = package_analyses
        .iter()
        .flat_map(|analysis| analysis.references.iter().cloned())
        .collect();
    resolve_references(&mut codebase, &references);

    for message in &model_errors {
        codebase.add_diagnostic(
            Diagnostic::error(message.clone(), None).with_code("analysis::model-invariant"),
        );
    }

    let analysis_metadata = codebase.analysis_mut();
    analysis_metadata.mode = AnalysisMode::SyntaxOnly;
    analysis_metadata.files_analyzed = files_analyzed;
    analysis_metadata.files_failed = files_failed;
    analysis_metadata.partial =
        files_failed > 0 || had_discovery_problems || !model_errors.is_empty();

    Ok(codebase)
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
