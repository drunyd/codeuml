use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use codeuml_analysis::{analyze_project, ProjectPath};
use codeuml_core::{
    Codebase, Relationship, RelationshipKind, RelationshipTarget, ResolutionStatus, TypeDecl,
};

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct TempProject {
    path: PathBuf,
}

impl TempProject {
    fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "codeuml-references-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temporary project");
        Self { path }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    fn write_package(&self, directory: &str, name: &str, source: &str) {
        let manifest = if directory.is_empty() {
            "Cargo.toml".to_owned()
        } else {
            format!("{directory}/Cargo.toml")
        };
        let source_path = if directory.is_empty() {
            "src/lib.rs".to_owned()
        } else {
            format!("{directory}/src/lib.rs")
        };
        self.write(
            &manifest,
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        );
        self.write(&source_path, source);
    }

    fn analyze(&self) -> Codebase {
        let project = ProjectPath::resolve(Some(&self.path)).expect("resolve project");
        analyze_project(&project).expect("analyze project")
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn type_by_qualified<'a>(codebase: &'a Codebase, qualified_name: &str) -> &'a TypeDecl {
    codebase
        .types()
        .iter()
        .find(|declaration| declaration.qualified_name == qualified_name)
        .unwrap_or_else(|| panic!("missing type {qualified_name}"))
}

fn relationships_for<'a>(
    codebase: &'a Codebase,
    source: &codeuml_core::SymbolId,
    kind: RelationshipKind,
) -> Vec<&'a Relationship> {
    codebase
        .relationships()
        .iter()
        .filter(|relationship| &relationship.source == source && relationship.kind == kind)
        .collect()
}

fn target_symbol(relationship: &Relationship) -> &codeuml_core::SymbolId {
    relationship
        .target
        .symbol()
        .unwrap_or_else(|| panic!("expected a resolved target: {relationship:?}"))
}

fn field<'a>(declaration: &'a TypeDecl, name: &str) -> &'a codeuml_core::Field {
    declaration
        .fields
        .iter()
        .find(|field| field.name == name)
        .unwrap_or_else(|| panic!("missing field {name}"))
}

fn method<'a>(declaration: &'a TypeDecl, name: &str) -> &'a codeuml_core::Method {
    declaration
        .methods
        .iter()
        .find(|method| method.name == name)
        .unwrap_or_else(|| panic!("missing method {name}"))
}

#[test]
fn resolves_direct_nested_and_compound_field_references() {
    let project = TempProject::new("basic");
    project.write_package(
        "",
        "refs",
        r#"
pub struct Customer;

pub struct Graph {
    pub direct: Customer,
    pub vec: Vec<Customer>,
    pub nested: Option<Box<Customer>>,
    pub reference: &'static mut Customer,
    pub tuple: (Customer, u32),
    pub array: [Customer; 2],
    pub slice: &'static [Customer],
    pub pointer: *const Customer,
    pub both: (Customer, Vec<Customer>),
    pub primitive: u32,
}

impl Graph {
    pub fn use_customer(&self, input: Customer) -> Customer {
        input
    }
}
"#,
    );

    let codebase = project.analyze();
    assert_eq!(
        codebase.types().len(),
        2,
        "no fake container or primitive types"
    );
    let customer = type_by_qualified(&codebase, "refs::Customer");
    let graph = type_by_qualified(&codebase, "refs::Graph");

    for name in [
        "direct",
        "vec",
        "nested",
        "reference",
        "tuple",
        "array",
        "slice",
        "pointer",
        "both",
    ] {
        let field = field(graph, name);
        let relationships = relationships_for(&codebase, &field.id, RelationshipKind::FieldType);
        let targets: Vec<_> = relationships.iter().map(|r| target_symbol(r)).collect();
        assert!(
            targets.contains(&&customer.id),
            "field {name} should reference Customer: {relationships:?}"
        );
        assert!(
            relationships
                .iter()
                .all(|relationship| relationship.resolution == ResolutionStatus::Inferred),
            "syntax resolution should be Inferred for field {name}"
        );
        assert!(
            relationships
                .iter()
                .flat_map(|relationship| &relationship.evidence)
                .all(|evidence| evidence.location.file == Path::new("src/lib.rs")),
            "evidence should retain the source file for field {name}"
        );
    }

    let both = relationships_for(
        &codebase,
        &field(graph, "both").id,
        RelationshipKind::FieldType,
    );
    assert_eq!(both.len(), 1, "nested repeats should merge into one edge");
    assert!(
        both[0].evidence.len() >= 2,
        "repeated occurrences should retain multiple evidence entries"
    );

    let primitive = relationships_for(
        &codebase,
        &field(graph, "primitive").id,
        RelationshipKind::FieldType,
    );
    assert!(primitive.is_empty());

    let use_customer = method(graph, "use_customer");
    let parameters = relationships_for(
        &codebase,
        &use_customer.id,
        RelationshipKind::MethodParameterType,
    );
    assert_eq!(parameters.len(), 1);
    assert_eq!(target_symbol(parameters[0]), &customer.id);
    let returns = relationships_for(
        &codebase,
        &use_customer.id,
        RelationshipKind::MethodReturnType,
    );
    assert_eq!(returns.len(), 1);
    assert_eq!(target_symbol(returns[0]), &customer.id);
    assert_eq!(
        parameters[0].evidence[0].location.line,
        use_customer.location.line
    );
}

#[test]
fn resolves_imports_qualified_paths_and_duplicate_short_names() {
    let project = TempProject::new("modules");
    project.write_package(
        "",
        "refs",
        r#"
pub mod a;
pub mod b;
pub mod c;

pub struct Root {
    pub via_crate: crate::a::Thing,
    pub via_module: a::Thing,
    pub via_self: self::a::Thing,
    pub a_shared: a::Shared,
    pub b_shared: b::Shared,
}
"#,
    );
    project.write("src/a.rs", "pub struct Thing;\npub struct Shared;\n");
    project.write("src/b.rs", "pub struct Shared;\n");
    project.write(
        "src/c.rs",
        r#"
use super::b::Shared as Renamed;
use crate::a::Thing;

pub struct Child {
    pub imported: Thing,
    pub renamed: Renamed,
    pub sibling: super::a::Thing,
}
"#,
    );

    let codebase = project.analyze();
    let root = type_by_qualified(&codebase, "refs::Root");
    let thing = type_by_qualified(&codebase, "refs::a::Thing");
    let a_shared = type_by_qualified(&codebase, "refs::a::Shared");
    let b_shared = type_by_qualified(&codebase, "refs::b::Shared");

    for name in ["via_crate", "via_module", "via_self"] {
        let relationships = relationships_for(
            &codebase,
            &field(root, name).id,
            RelationshipKind::FieldType,
        );
        assert_eq!(relationships.len(), 1, "field {name}");
        assert_eq!(target_symbol(relationships[0]), &thing.id, "field {name}");
    }
    assert_eq!(
        target_symbol(
            relationships_for(
                &codebase,
                &field(root, "a_shared").id,
                RelationshipKind::FieldType,
            )[0]
        ),
        &a_shared.id
    );
    assert_eq!(
        target_symbol(
            relationships_for(
                &codebase,
                &field(root, "b_shared").id,
                RelationshipKind::FieldType,
            )[0]
        ),
        &b_shared.id
    );

    let child = type_by_qualified(&codebase, "refs::c::Child");
    for name in ["imported", "sibling"] {
        let relationships = relationships_for(
            &codebase,
            &field(child, name).id,
            RelationshipKind::FieldType,
        );
        assert_eq!(target_symbol(relationships[0]), &thing.id, "field {name}");
    }
    assert_eq!(
        target_symbol(
            relationships_for(
                &codebase,
                &field(child, "renamed").id,
                RelationshipKind::FieldType,
            )[0]
        ),
        &b_shared.id
    );
}

#[test]
fn preserves_ambiguous_and_unresolved_references_and_ignores_external_heads() {
    let project = TempProject::new("unresolved");
    project.write_package(
        "",
        "refs",
        r#"
pub mod a;
pub mod b;

use a::*;
use b::*;
use std::collections::HashMap;

pub struct Holder {
    pub duplicate: Duplicate,
    pub missing: MissingType,
    pub external: serde::Serialize,
    pub vector: Vec<u32>,
    pub map: HashMap<String, Duplicate>,
    pub primitive: bool,
}
"#,
    );
    project.write("src/a.rs", "pub struct Duplicate;\n");
    project.write("src/b.rs", "pub struct Duplicate;\n");

    let codebase = project.analyze();
    let holder = type_by_qualified(&codebase, "refs::Holder");

    let duplicate = relationships_for(
        &codebase,
        &field(holder, "duplicate").id,
        RelationshipKind::FieldType,
    );
    assert_eq!(duplicate.len(), 1);
    assert_eq!(duplicate[0].resolution, ResolutionStatus::Unresolved);
    assert_eq!(
        duplicate[0].target,
        RelationshipTarget::Unresolved(codeuml_core::UnresolvedReference::new("Duplicate"))
    );
    assert!(codebase.diagnostics().iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("analysis::ambiguous-reference")
            && diagnostic.message.contains("Duplicate")
    }));

    let missing = relationships_for(
        &codebase,
        &field(holder, "missing").id,
        RelationshipKind::FieldType,
    );
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].target,
        RelationshipTarget::Unresolved(codeuml_core::UnresolvedReference::new("MissingType"))
    );

    let external = relationships_for(
        &codebase,
        &field(holder, "external").id,
        RelationshipKind::FieldType,
    );
    assert_eq!(external.len(), 1, "unknown external paths remain explicit");
    assert_eq!(external[0].resolution, ResolutionStatus::Unresolved);

    assert!(relationships_for(
        &codebase,
        &field(holder, "vector").id,
        RelationshipKind::FieldType,
    )
    .is_empty());
    assert!(relationships_for(
        &codebase,
        &field(holder, "primitive").id,
        RelationshipKind::FieldType,
    )
    .is_empty());

    let map = relationships_for(
        &codebase,
        &field(holder, "map").id,
        RelationshipKind::FieldType,
    );
    assert_eq!(map.len(), 1, "nested duplicate remains unresolved");
    assert_eq!(map[0].resolution, ResolutionStatus::Unresolved);
}

#[test]
fn resolves_alias_chains_and_alias_targets() {
    let project = TempProject::new("aliases");
    project.write_package(
        "",
        "refs",
        r#"
pub struct Customer;
pub type CustomerAlias = Customer;
pub type DeepAlias = Vec<CustomerAlias>;

pub struct Holder {
    pub customer: CustomerAlias,
    pub deep: DeepAlias,
}
"#,
    );

    let codebase = project.analyze();
    let customer = type_by_qualified(&codebase, "refs::Customer");
    let alias = type_by_qualified(&codebase, "refs::CustomerAlias");
    let deep = type_by_qualified(&codebase, "refs::DeepAlias");

    let alias_targets = relationships_for(&codebase, &alias.id, RelationshipKind::Aliases);
    assert_eq!(alias_targets.len(), 1);
    assert_eq!(target_symbol(alias_targets[0]), &customer.id);
    let deep_targets = relationships_for(&codebase, &deep.id, RelationshipKind::Aliases);
    assert_eq!(deep_targets.len(), 1);
    assert_eq!(target_symbol(deep_targets[0]), &alias.id);

    let holder = type_by_qualified(&codebase, "refs::Holder");
    assert_eq!(
        target_symbol(
            relationships_for(
                &codebase,
                &field(holder, "customer").id,
                RelationshipKind::FieldType,
            )[0]
        ),
        &alias.id
    );
    assert_eq!(
        target_symbol(
            relationships_for(
                &codebase,
                &field(holder, "deep").id,
                RelationshipKind::FieldType,
            )[0]
        ),
        &deep.id
    );
}

#[test]
fn extracts_trait_bounds_implementations_and_qualified_associated_types() {
    let project = TempProject::new("traits");
    project.write_package(
        "",
        "refs",
        r#"
pub trait Base {
    type Item;
}

pub trait Child: Base {
    fn make<T: Base>(&self, input: T) -> Self::Item;
}

pub struct Customer;

impl Base for Customer {
    type Item = Customer;
}

pub struct Holder<T: Base>
where
    T: Base,
{
    pub value: T,
    pub item: <T as Base>::Item,
}
"#,
    );

    let codebase = project.analyze();
    let base = type_by_qualified(&codebase, "refs::Base");
    let child = type_by_qualified(&codebase, "refs::Child");
    let customer = type_by_qualified(&codebase, "refs::Customer");
    let holder = type_by_qualified(&codebase, "refs::Holder");

    let implements = relationships_for(&codebase, &customer.id, RelationshipKind::Implements);
    assert_eq!(implements.len(), 1);
    assert_eq!(target_symbol(implements[0]), &base.id);

    let supertrait = relationships_for(&codebase, &child.id, RelationshipKind::TraitBound);
    assert!(
        supertrait
            .iter()
            .any(|relationship| target_symbol(relationship) == &base.id),
        "trait supertrait should produce a bound: {supertrait:?}"
    );
    let holder_bounds = relationships_for(&codebase, &holder.id, RelationshipKind::TraitBound);
    assert_eq!(
        holder_bounds
            .iter()
            .filter(|relationship| target_symbol(relationship) == &base.id)
            .count(),
        1,
        "inline and where bounds merge into one edge"
    );

    let make = method(child, "make");
    let method_bounds = relationships_for(&codebase, &make.id, RelationshipKind::TraitBound);
    assert!(method_bounds
        .iter()
        .any(|relationship| target_symbol(relationship) == &base.id));

    let item = relationships_for(
        &codebase,
        &field(holder, "item").id,
        RelationshipKind::FieldType,
    );
    assert!(item.iter().any(|relationship| {
        relationship.resolution == ResolutionStatus::Unresolved
            && relationship
                .target
                .unresolved()
                .is_some_and(|reference| reference.name.contains("<T as Base>::Item"))
    }));
    assert!(!codebase
        .types()
        .iter()
        .any(|declaration| declaration.name == "T"));
}

#[test]
fn resolves_cross_crate_workspace_references() {
    let project = TempProject::new("workspace");
    project.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"domain\", \"app\"]\nresolver = \"2\"\n",
    );
    project.write_package("domain", "domain", "pub struct Customer;\n");
    project.write(
        "app/Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndomain = { path = \"../domain\" }\n",
    );
    project.write(
        "app/src/lib.rs",
        r#"
use domain::Customer;

pub struct Order {
    pub direct: domain::Customer,
    pub imported: Customer,
    pub nested: Vec<Customer>,
}
"#,
    );

    let codebase = project.analyze();
    let customer = type_by_qualified(&codebase, "domain::Customer");
    let order = type_by_qualified(&codebase, "app::Order");

    for name in ["direct", "imported", "nested"] {
        let relationships = relationships_for(
            &codebase,
            &field(order, name).id,
            RelationshipKind::FieldType,
        );
        assert_eq!(relationships.len(), 1, "field {name}");
        assert_eq!(
            target_symbol(relationships[0]),
            &customer.id,
            "field {name}"
        );
    }
}

#[test]
fn valid_references_survive_parse_failures_in_other_files() {
    let project = TempProject::new("partial");
    project.write_package(
        "",
        "refs",
        r#"
pub mod broken;

pub struct Customer;
pub struct Holder {
    pub customer: Customer,
}
"#,
    );
    project.write("src/broken.rs", "pub struct Broken { this is not Rust\n");

    let codebase = project.analyze();
    let customer = type_by_qualified(&codebase, "refs::Customer");
    let holder = type_by_qualified(&codebase, "refs::Holder");
    assert_eq!(
        target_symbol(
            relationships_for(
                &codebase,
                &field(holder, "customer").id,
                RelationshipKind::FieldType,
            )[0]
        ),
        &customer.id
    );
    assert_eq!(codebase.analysis().files_failed, 1);
}

#[test]
fn reference_results_are_stable_across_repeated_analysis() {
    let project = TempProject::new("stable");
    project.write_package(
        "",
        "refs",
        r#"
pub struct Customer;
pub struct Holder {
    pub first: Customer,
    pub second: Vec<Customer>,
}
"#,
    );

    let first = project.analyze();
    let second = project.analyze();

    assert_eq!(first.relationships(), second.relationships());
    assert_eq!(first.diagnostics(), second.diagnostics());
}
