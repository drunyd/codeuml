//! [`LanguageProvider`] for Rust: Cargo discovery, syntax analysis, and
//! conservative reference resolution behind the language-neutral contract.

use codeuml_core::{Codebase, Diagnostic, Language, SourceFile};
use codeuml_lang::{LanguageProvider, ProjectPath, ProviderError, ProviderReport};

use crate::resolution::resolve_references;
use crate::{
    analyze_package, discover_project, DiscoveredPackage, DiscoveredProject, RustAnalysis,
    RustPackageSources, RustReference, RustTargetSource,
};

const PROVIDER_ID: &str = "rust";
const MANIFEST: &str = "Cargo.toml";

/// Analyses Cargo projects with syntax-level Rust analysis.
///
/// Discovery and each package analyzer run independently; per-file parse
/// failures and recoverable discovery problems become diagnostics rather than
/// discarding the rest of the model.
#[derive(Clone, Copy, Debug, Default)]
pub struct RustProvider;

impl LanguageProvider for RustProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn detection_hint(&self) -> &'static str {
        "Rust (Cargo.toml)"
    }

    fn detect(&self, root: &ProjectPath) -> bool {
        root.as_path().join(MANIFEST).is_file()
    }

    fn analyze(
        &self,
        root: &ProjectPath,
        codebase: &mut Codebase,
    ) -> Result<ProviderReport, ProviderError> {
        let discovered =
            discover_project(root).map_err(|error| ProviderError::new(PROVIDER_ID, error))?;

        add_discovered_files(codebase, &discovered);
        let had_discovery_problems = add_discovery_diagnostics(codebase, &discovered);
        let package_analyses = analyze_packages(&discovered);
        let merged = merge_package_analyses(codebase, &package_analyses);
        resolve_references(codebase, &collect_references(&package_analyses));
        add_model_error_diagnostics(codebase, &merged.model_errors);

        Ok(ProviderReport {
            files_analyzed: merged.files_analyzed,
            files_failed: merged.files_failed,
            partial: merged.files_failed > 0
                || had_discovery_problems
                || !merged.model_errors.is_empty(),
        })
    }
}

fn add_discovered_files(codebase: &mut Codebase, discovered: &DiscoveredProject) {
    for file in discovered.source_files() {
        codebase.add_file(SourceFile::new(file, Language::Rust));
    }
}

fn add_discovery_diagnostics(codebase: &mut Codebase, discovered: &DiscoveredProject) -> bool {
    let diagnostics = discovered.diagnostics();
    for diagnostic in diagnostics {
        codebase.add_diagnostic(diagnostic.clone());
    }
    !diagnostics.is_empty()
}

fn analyze_packages(discovered: &DiscoveredProject) -> Vec<RustAnalysis> {
    discovered
        .packages()
        .iter()
        .map(rust_package_sources)
        .map(|package| analyze_package(discovered.root(), &package))
        .collect()
}

#[derive(Default)]
struct Merged {
    files_analyzed: usize,
    files_failed: usize,
    model_errors: Vec<String>,
}

fn merge_package_analyses(codebase: &mut Codebase, analyses: &[RustAnalysis]) -> Merged {
    let mut merged = Merged::default();
    for analysis in analyses {
        merged.files_analyzed += analysis.files_analyzed;
        merged.files_failed += analysis.files_failed;
        add_package_diagnostics(codebase, analysis);
        add_package_modules(codebase, analysis, &mut merged.model_errors);
        add_package_types(codebase, analysis, &mut merged.model_errors);
    }
    merged
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
