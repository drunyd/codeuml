//! Orchestration behaviour, exercised with fake providers so it is proven
//! independent of any real language.

use std::cell::Cell;
use std::rc::Rc;

use codeuml_analysis::{
    analyze_project, AnalysisError, LanguageProvider, ProjectPath, ProviderError, ProviderReport,
};
use codeuml_core::{Codebase, DiagnosticSeverity, Module, SourceLocation, SymbolId, TypeDecl};
use codeuml_core::{Language, SourceFile, TypeKind};

enum Outcome {
    Succeed(ProviderReport),
    Fail(&'static str),
}

struct Fake {
    id: &'static str,
    detects: bool,
    outcome: Outcome,
    analyzed: Rc<Cell<bool>>,
}

impl Fake {
    fn new(id: &'static str, detects: bool, outcome: Outcome) -> Self {
        Self {
            id,
            detects,
            outcome,
            analyzed: Rc::new(Cell::new(false)),
        }
    }
}

impl LanguageProvider for Fake {
    fn id(&self) -> &'static str {
        self.id
    }

    fn detection_hint(&self) -> &'static str {
        self.id
    }

    fn detect(&self, _root: &ProjectPath) -> bool {
        self.detects
    }

    fn analyze(
        &self,
        _root: &ProjectPath,
        codebase: &mut Codebase,
    ) -> Result<ProviderReport, ProviderError> {
        self.analyzed.set(true);
        let report = match &self.outcome {
            Outcome::Fail(reason) => return Err(ProviderError::new(self.id, reason)),
            Outcome::Succeed(report) => *report,
        };
        let file = format!("src/{}.fake", self.id);
        let location = SourceLocation::new(&file, 1, 1, 1, 1);
        let module = SymbolId::declaration(&file, self.id);
        codebase.add_file(SourceFile::new(&file, Language::Other(self.id.to_owned())));
        codebase
            .add_module(Module::new(
                module.clone(),
                self.id,
                self.id,
                &file,
                location.clone(),
            ))
            .expect("unique module");
        let type_id = SymbolId::declaration(&file, &format!("{}::Thing", self.id));
        codebase
            .add_type(
                TypeDecl::new(
                    type_id,
                    "Thing",
                    format!("{}::Thing", self.id),
                    TypeKind::Class,
                    location,
                )
                .with_module(module),
            )
            .expect("unique type");
        Ok(report)
    }
}

fn report(analyzed: usize, failed: usize, partial: bool) -> Outcome {
    Outcome::Succeed(ProviderReport {
        files_analyzed: analyzed,
        files_failed: failed,
        partial,
    })
}

fn project() -> ProjectPath {
    ProjectPath::resolve(Some(&std::env::temp_dir())).expect("temp dir")
}

fn boxed(providers: Vec<Fake>) -> Vec<Box<dyn LanguageProvider>> {
    providers
        .into_iter()
        .map(|provider| Box::new(provider) as Box<dyn LanguageProvider>)
        .collect()
}

#[test]
fn only_providers_that_detect_the_project_run_and_share_one_model() {
    let python = Fake::new("python", true, report(3, 0, false));
    let skipped = Fake::new("java", false, report(9, 9, true));
    let rust = Fake::new("rust", true, report(4, 1, false));
    let (python_ran, java_ran, rust_ran) = (
        python.analyzed.clone(),
        skipped.analyzed.clone(),
        rust.analyzed.clone(),
    );

    let codebase =
        analyze_project(&project(), &boxed(vec![python, skipped, rust])).expect("analysis");

    assert!(python_ran.get() && rust_ran.get() && !java_ran.get());
    assert_eq!(codebase.modules().len(), 2);
    assert_eq!(codebase.types().len(), 2);
    assert_eq!(codebase.files().len(), 2);
    assert_eq!(codebase.analysis().files_analyzed, 7);
    assert_eq!(codebase.analysis().files_failed, 1);
    assert!(codebase.analysis().partial, "failed files imply partial");
    assert!(codebase.diagnostics().is_empty());
}

#[test]
fn a_partial_provider_report_makes_the_model_partial() {
    let providers = boxed(vec![Fake::new("python", true, report(1, 0, true))]);
    let codebase = analyze_project(&project(), &providers).expect("analysis");
    assert!(codebase.analysis().partial);
    assert_eq!(codebase.analysis().files_failed, 0);

    let providers = boxed(vec![Fake::new("python", true, report(1, 0, false))]);
    let codebase = analyze_project(&project(), &providers).expect("analysis");
    assert!(!codebase.analysis().partial);
}

#[test]
fn one_failing_provider_degrades_to_a_diagnostic_when_another_succeeds() {
    let providers = boxed(vec![
        Fake::new("rust", true, Outcome::Fail("broken manifest")),
        Fake::new("python", true, report(2, 0, false)),
    ]);

    let codebase = analyze_project(&project(), &providers).expect("analysis survives");

    assert_eq!(codebase.types().len(), 1);
    assert!(codebase.analysis().partial);
    let diagnostics = codebase.diagnostics();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
    assert_eq!(
        diagnostics[0].code.as_deref(),
        Some("analysis::provider-failed")
    );
    assert!(diagnostics[0]
        .message
        .contains("rust analysis failed: broken manifest"));
}

#[test]
fn analysis_fails_with_the_first_error_when_every_provider_fails() {
    let providers = boxed(vec![
        Fake::new("rust", true, Outcome::Fail("first")),
        Fake::new("python", true, Outcome::Fail("second")),
    ]);

    let error = analyze_project(&project(), &providers).expect_err("all failed");

    assert!(
        matches!(&error, AnalysisError::Provider(provider) if provider.provider() == "rust"),
        "{error}"
    );
    assert_eq!(error.to_string(), "first");
}

#[test]
fn unrecognised_projects_list_what_each_language_looks_for() {
    let providers = boxed(vec![
        Fake::new("rust", false, report(0, 0, false)),
        Fake::new("python", false, report(0, 0, false)),
    ]);

    let error = analyze_project(&project(), &providers).expect_err("nothing detected");

    assert!(matches!(error, AnalysisError::NoSupportedProject { .. }));
    assert!(
        error.to_string().contains("looked for: rust, python"),
        "{error}"
    );
}
