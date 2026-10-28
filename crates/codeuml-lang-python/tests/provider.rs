use std::fs;
use std::path::PathBuf;

use codeuml_core::{
    Codebase, DiagnosticSeverity, RelationshipKind, RelationshipTarget, ResolutionStatus, SymbolId,
    TypeKind,
};
use codeuml_lang::{LanguageProvider, ProjectPath, ProviderReport};
use codeuml_lang_python::PythonProvider;

struct TempProject(PathBuf);

impl TempProject {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codeuml-python-provider-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path).expect("temp project");
        Self(path)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("directories");
        fs::write(path, contents).expect("write");
    }

    fn root(&self) -> ProjectPath {
        ProjectPath::resolve(Some(&self.0)).expect("project path")
    }

    fn analyze(&self) -> (Codebase, ProviderReport) {
        let mut codebase = Codebase::new("fixture");
        let report = PythonProvider
            .analyze(&self.root(), &mut codebase)
            .expect("analysis");
        (codebase, report)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn shop_project() -> TempProject {
    let project = TempProject::new("shop");
    project.write("pyproject.toml", "[project]\nname = \"shop\"\n");
    project.write("src/shop/__init__.py", "from .models import Customer\n");
    project.write(
        "src/shop/models.py",
        "from abc import ABC\nfrom typing import List, Optional\n\nclass Entity(ABC):\n    id: int\n\nclass Customer(Entity):\n    name: str\n\n    def __init__(self, name: str, partner: \"Customer | None\" = None) -> None:\n        self.partner = partner\n\n    def orders(self) -> List[\"Order\"]: ...\n\nclass Order:\n    owner: Customer\n\n    def total(self, tax: Tax) -> float: ...\n",
    );
    project.write(
        "src/shop/billing.py",
        "from .models import Order\nfrom shop.models import Missing\n\nclass Invoice(Order):\n    def line(self) -> Missing: ...\n",
    );
    project.write("tests/test_models.py", "class TestOnly: ...\n");
    project
}

fn type_id(codebase: &Codebase, qualified: &str) -> SymbolId {
    codebase
        .types()
        .iter()
        .find(|declaration| declaration.qualified_name == qualified)
        .unwrap_or_else(|| panic!("missing type {qualified}"))
        .id
        .clone()
}

fn edges(
    codebase: &Codebase,
    source: &SymbolId,
    kind: RelationshipKind,
) -> Vec<RelationshipTarget> {
    codebase
        .relationships()
        .iter()
        .filter(|relationship| &relationship.source == source && relationship.kind == kind)
        .map(|relationship| relationship.target.clone())
        .collect()
}

#[test]
fn identifies_itself_and_what_it_looks_for() {
    assert_eq!(PythonProvider.id(), "python");
    assert!(PythonProvider.detection_hint().contains("pyproject.toml"));
}

#[test]
fn detection_follows_markers_and_python_files() {
    let empty = TempProject::new("empty");
    assert!(!PythonProvider.detect(&empty.root()));
    empty.write("requirements.txt", "requests\n");
    assert!(PythonProvider.detect(&empty.root()));

    let scripts = TempProject::new("scripts");
    scripts.write("tool.py", "");
    assert!(PythonProvider.detect(&scripts.root()));
}

#[test]
fn a_src_layout_project_becomes_modules_types_and_members() {
    let (codebase, report) = shop_project().analyze();

    assert_eq!((report.files_analyzed, report.files_failed), (3, 0));
    assert!(!report.partial);
    let modules: Vec<_> = codebase
        .modules()
        .iter()
        .map(|module| module.qualified_name.as_str())
        .collect();
    assert_eq!(modules, ["shop", "shop.billing", "shop.models"]);
    let shop = codebase
        .modules()
        .iter()
        .find(|module| module.qualified_name == "shop")
        .expect("package");
    assert!(shop.parent.is_none());
    assert_eq!(
        shop.file,
        PathBuf::from("src").join("shop").join("__init__.py")
    );
    let models = codebase
        .modules()
        .iter()
        .find(|module| module.qualified_name == "shop.models")
        .expect("module");
    assert_eq!(models.parent.as_ref(), Some(&shop.id));

    let names: Vec<_> = codebase
        .types()
        .iter()
        .map(|declaration| declaration.qualified_name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "shop.billing.Invoice",
            "shop.models.Entity",
            "shop.models.Customer",
            "shop.models.Order"
        ]
    );
    let entity = codebase
        .type_by_id(&type_id(&codebase, "shop.models.Entity"))
        .expect("entity");
    assert_eq!(entity.kind, TypeKind::Interface);
    let customer = codebase
        .type_by_id(&type_id(&codebase, "shop.models.Customer"))
        .expect("customer");
    assert_eq!(customer.kind, TypeKind::Class);
    let field_names: Vec<_> = customer.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(field_names, ["name", "partner"]);
    assert_eq!(
        customer.fields[1].type_name.as_deref(),
        Some("Customer | None")
    );
}

#[test]
fn bases_and_annotations_become_resolved_relationships() {
    let (codebase, _) = shop_project().analyze();
    let customer = type_id(&codebase, "shop.models.Customer");
    let order = type_id(&codebase, "shop.models.Order");
    let entity = type_id(&codebase, "shop.models.Entity");
    let invoice = type_id(&codebase, "shop.billing.Invoice");

    // A base that is an ABC is an interface, so the edge is `implements`.
    assert_eq!(
        edges(&codebase, &customer, RelationshipKind::Implements),
        [RelationshipTarget::Symbol(entity)]
    );
    assert_eq!(
        edges(&codebase, &invoice, RelationshipKind::Extends),
        [RelationshipTarget::Symbol(order.clone())]
    );
    let partner = SymbolId::field(&customer, "partner");
    assert_eq!(
        edges(&codebase, &partner, RelationshipKind::FieldType),
        [RelationshipTarget::Symbol(customer.clone())]
    );
    let orders = SymbolId::method(&customer, "orders");
    assert_eq!(
        edges(&codebase, &orders, RelationshipKind::MethodReturnType),
        [RelationshipTarget::Symbol(order.clone())]
    );
    let owner = SymbolId::field(&order, "owner");
    assert_eq!(
        edges(&codebase, &owner, RelationshipKind::FieldType),
        [RelationshipTarget::Symbol(customer)]
    );
}

#[test]
fn unresolvable_names_are_kept_and_project_internal_ones_are_diagnosed() {
    let (codebase, report) = shop_project().analyze();
    let order = type_id(&codebase, "shop.models.Order");
    let invoice = type_id(&codebase, "shop.billing.Invoice");

    let total = SymbolId::method(&order, "total");
    let tax = edges(&codebase, &total, RelationshipKind::MethodParameterType);
    assert_eq!(tax.len(), 1);
    assert!(
        matches!(&tax[0], RelationshipTarget::Unresolved(reference) if reference.name == "Tax")
    );
    // An unknown bare name is preserved but not reported; a name imported from
    // a project module that lacks it is.
    let line = SymbolId::method(&invoice, "line");
    let missing = codebase
        .relationships()
        .iter()
        .find(|relationship| relationship.source == line)
        .expect("missing edge");
    assert_eq!(missing.resolution, ResolutionStatus::Unresolved);
    let warnings: Vec<_> = codebase
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code.as_deref() == Some("analysis::unresolved-reference"))
        .collect();
    assert_eq!(warnings.len(), 1, "{:?}", codebase.diagnostics());
    assert_eq!(warnings[0].severity, DiagnosticSeverity::Warning);
    assert!(warnings[0].message.contains("Missing"));
    assert!(warnings[0].location.is_some());
    assert!(!report.partial);
}

#[test]
fn a_file_that_does_not_parse_is_reported_and_the_rest_still_analyzed() {
    let project = shop_project();
    project.write("src/shop/legacy.py", "def broken(:\n    pass\n");
    let (codebase, report) = project.analyze();

    assert_eq!((report.files_analyzed, report.files_failed), (3, 1));
    assert!(report.partial);
    let failure = codebase
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code.as_deref() == Some("python::parse-error"))
        .expect("parse diagnostic");
    assert_eq!(failure.severity, DiagnosticSeverity::Error);
    let location = failure.location.as_ref().expect("location");
    assert_eq!(
        location.file,
        PathBuf::from("src").join("shop").join("legacy.py")
    );
    assert_eq!(location.line, 1);
    assert_eq!(codebase.types().len(), 4);
}

#[test]
fn unreadable_and_oversized_sources_fail_individually() {
    let project = TempProject::new("unreadable");
    project.write("pyproject.toml", "");
    project.write("ok.py", "class Fine: ...\n");
    fs::write(project.0.join("latin.py"), [0xff, 0xfe, b'x']).expect("binary file");
    let (codebase, report) = project.analyze();
    assert_eq!((report.files_analyzed, report.files_failed), (1, 1));
    assert!(codebase
        .diagnostics()
        .iter()
        .any(|d| d.code.as_deref() == Some("python::unreadable-source")));

    let big = TempProject::new("big");
    big.write("pyproject.toml", "");
    let file = fs::File::create(big.0.join("huge.py")).expect("file");
    file.set_len(6 * 1024 * 1024).expect("sparse file");
    let (codebase, report) = big.analyze();
    assert_eq!(report.files_failed, 1);
    assert!(codebase
        .diagnostics()
        .iter()
        .any(|d| d.code.as_deref() == Some("python::source-too-large")));
}

#[test]
fn projects_without_python_sources_yield_an_empty_complete_model() {
    let project = TempProject::new("markeronly");
    project.write("pyproject.toml", "[project]\nname = \"x\"\n");
    let (codebase, report) = project.analyze();
    assert_eq!((report.files_analyzed, report.files_failed), (0, 0));
    assert!(!report.partial);
    assert!(codebase.types().is_empty() && codebase.modules().is_empty());
}

#[test]
fn many_files_are_parsed_concurrently_but_reported_in_a_stable_order() {
    let project = TempProject::new("parallel");
    project.write("pyproject.toml", "");
    for index in 0..60 {
        project.write(
            &format!("pkg/m{index:02}.py"),
            &format!("class C{index:02}:\n    x: int\n"),
        );
    }
    project.write("pkg/m17x.py", "def broken(:\n");
    project.write("pkg/m33x.py", "def broken(:\n");

    let (first, report) = project.analyze();
    let (second, _) = project.analyze();

    assert_eq!((report.files_analyzed, report.files_failed), (60, 2));
    let names = |codebase: &Codebase| -> Vec<String> {
        codebase
            .types()
            .iter()
            .map(|declaration| declaration.qualified_name.clone())
            .collect()
    };
    let expected: Vec<String> = (0..60)
        .map(|index| format!("pkg.m{index:02}.C{index:02}"))
        .collect();
    assert_eq!(names(&first), expected);
    assert_eq!(names(&second), expected);
    let failures: Vec<_> = first
        .diagnostics()
        .iter()
        .filter_map(|d| d.location.as_ref().map(|l| l.file.clone()))
        .collect();
    assert_eq!(
        failures,
        [
            PathBuf::from("pkg").join("m17x.py"),
            PathBuf::from("pkg").join("m33x.py")
        ]
    );
}

#[test]
fn a_class_defined_twice_under_one_module_name_makes_references_ambiguous() {
    // `a.py` and `a/__init__.py` both define the module `a`.
    let project = TempProject::new("collision");
    project.write("pyproject.toml", "");
    project.write("a.py", "class Same: ...\n");
    project.write("a/__init__.py", "class Same: ...\n");
    project.write("b.py", "from a import Same\nclass User(Same): ...\n");
    let (codebase, report) = project.analyze();

    assert_eq!(report.files_failed, 0);
    assert_eq!(codebase.types().len(), 3);
    let user = type_id(&codebase, "b.User");
    let extends = edges(&codebase, &user, RelationshipKind::Extends);
    assert!(
        matches!(&extends[..], [RelationshipTarget::Unresolved(reference)] if reference.name == "Same")
    );
    assert!(codebase.diagnostics().iter().any(|d| d.code.as_deref()
        == Some("analysis::ambiguous-reference")
        && d.message.contains("a.Same")));
}
