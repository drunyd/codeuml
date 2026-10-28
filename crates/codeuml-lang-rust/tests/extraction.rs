use std::fs;
use std::path::{Path, PathBuf};

use codeuml_core::{
    DiagnosticSeverity, Method, Module, RelationshipKind, TypeDecl, TypeKind, Visibility,
};
use codeuml_lang_rust::{analyze_package, RustAnalysis, RustPackageSources, RustTargetSource};

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn collect_rust_sources(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read fixture directory") {
            let path = entry.expect("read fixture entry").path();
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path.strip_prefix(root).expect("fixture-relative path");
                files.push(PathBuf::from(relative.to_string_lossy().replace('\\', "/")));
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort();
    files
}

fn analyze_fixture(name: &str) -> RustAnalysis {
    analyze_fixture_with_target(name, name, "src/lib.rs", true)
}

fn analyze_fixture_with_target(
    name: &str,
    target_name: &str,
    source: &str,
    is_library: bool,
) -> RustAnalysis {
    let root = fixture_root(name);
    let package = RustPackageSources {
        name: name.to_owned(),
        source_files: collect_rust_sources(&root),
        targets: vec![RustTargetSource {
            name: target_name.to_owned(),
            source_path: PathBuf::from(source),
            is_library,
        }],
    };
    analyze_package(&root, &package)
}

fn type_named<'a>(analysis: &'a RustAnalysis, name: &str) -> &'a TypeDecl {
    let matches: Vec<_> = analysis
        .types
        .iter()
        .filter(|declaration| declaration.name == name)
        .collect();
    assert_eq!(matches.len(), 1, "expected exactly one type named {name}");
    matches[0]
}

fn module_qualified<'a>(analysis: &'a RustAnalysis, qualified_name: &str) -> &'a Module {
    analysis
        .modules
        .iter()
        .find(|module| module.qualified_name == qualified_name)
        .unwrap_or_else(|| panic!("missing module {qualified_name}"))
}

fn method_named<'a>(declaration: &'a TypeDecl, name: &str) -> &'a Method {
    declaration
        .methods
        .iter()
        .find(|method| method.name == name)
        .unwrap_or_else(|| panic!("missing method {name}"))
}

#[test]
fn extracts_struct_shapes_fields_variants_and_aliases() {
    let analysis = analyze_fixture("shapes");

    assert_eq!(analysis.files_analyzed, 1);
    assert_eq!(analysis.files_failed, 0);
    assert!(analysis.diagnostics.is_empty());

    let named = type_named(&analysis, "Named");
    assert_eq!(named.kind, TypeKind::Struct);
    assert_eq!(named.location.file, PathBuf::from("src/lib.rs"));
    assert_eq!(named.location.line, 1);
    assert_eq!(named.fields.len(), 2);
    assert_eq!(named.fields[0].name, "id");
    assert_eq!(named.fields[0].type_name.as_deref(), Some("u64"));
    assert_eq!(named.fields[0].visibility, Visibility::Public);
    assert_eq!(named.fields[0].location.line, 2);
    assert!(named.fields[0].location.column > 0);
    assert_eq!(named.fields[1].name, "name");
    assert_eq!(named.fields[1].type_name.as_deref(), Some("String"));
    assert_eq!(named.fields[1].visibility, Visibility::Private);

    let tuple = type_named(&analysis, "Tuple");
    assert_eq!(tuple.location.line, 6);
    assert_eq!(tuple.fields.len(), 2);
    assert_eq!(tuple.fields[0].name, "0");
    assert_eq!(tuple.fields[0].type_name.as_deref(), Some("u32"));
    assert_eq!(tuple.fields[0].visibility, Visibility::Public);
    assert_eq!(tuple.fields[1].name, "1");
    assert_eq!(tuple.fields[1].type_name.as_deref(), Some("String"));

    let unit = type_named(&analysis, "Unit");
    assert_eq!(unit.location.line, 8);
    assert!(unit.fields.is_empty());

    let status = type_named(&analysis, "Status");
    assert_eq!(status.kind, TypeKind::Enum);
    assert_eq!(status.location.line, 10);
    let variant_names: Vec<_> = status.variants.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(variant_names, vec!["Active", "Suspended", "Pair", "Empty"]);
    assert_eq!(status.variants[1].fields[0].name, "reason");
    assert_eq!(
        status.variants[1].fields[0].type_name.as_deref(),
        Some("String")
    );
    assert_eq!(status.variants[2].fields[0].name, "0");
    assert_eq!(status.variants[2].fields[1].name, "1");
    assert!(status.variants[3].fields.is_empty());

    let bits = type_named(&analysis, "Bits");
    assert_eq!(bits.kind, TypeKind::Union);
    assert_eq!(bits.location.line, 17);
    assert_eq!(bits.fields.len(), 2);

    let alias = type_named(&analysis, "Alias");
    assert_eq!(alias.kind, TypeKind::TypeAlias);
    assert_eq!(alias.alias_of.as_deref(), Some("Vec<T>"));
    assert_eq!(alias.generics.len(), 1);
    assert_eq!(alias.generics[0].name, "T");
    assert_eq!(alias.location.line, 22);
}

#[test]
fn extracts_traits_inherent_methods_and_trait_implementations() {
    let analysis = analyze_fixture("traits");
    assert_eq!(analysis.files_failed, 0);
    assert!(analysis.diagnostics.is_empty());

    let entity = type_named(&analysis, "Entity");
    assert_eq!(entity.kind, TypeKind::Trait);
    assert_eq!(entity.methods.len(), 3);
    let create = method_named(entity, "create");
    assert!(create.is_static);
    assert_eq!(create.visibility, Visibility::Public);
    assert_eq!(create.return_type.as_deref(), Some("Self"));
    assert_eq!(create.parameters[0].name, "name");
    assert_eq!(create.parameters[0].type_name, "String");
    assert!(!method_named(entity, "id").is_static);
    assert_eq!(entity.methods[0].location.line, 2);

    let user = type_named(&analysis, "User");
    assert_eq!(user.fields.len(), 1);
    assert_eq!(user.methods.len(), 4);
    let new = method_named(user, "new");
    assert_eq!(new.location.line, 14);
    assert_eq!(new.return_type.as_deref(), Some("Self"));
    assert_eq!(new.parameters[0].name, "id");
    assert_eq!(new.parameters[0].type_name, "u64");
    assert_eq!(new.visibility, Visibility::Public);

    let id_methods: Vec<_> = user
        .methods
        .iter()
        .filter(|method| method.name == "id")
        .collect();
    assert_eq!(id_methods.len(), 2);
    assert_ne!(id_methods[0].id, id_methods[1].id);
    assert!(user
        .methods
        .iter()
        .any(|method| method.name == "create" && method.is_static));

    let user_implements = analysis
        .references
        .iter()
        .find(|reference| {
            reference.source == user.id && reference.kind == RelationshipKind::Implements
        })
        .expect("User implements Entity");
    assert_eq!(user_implements.path.display, "Entity");
    assert_eq!(user_implements.location.file, PathBuf::from("src/lib.rs"));
    assert_eq!(entity.name, "Entity");

    let widget = type_named(&analysis, "Widget");
    let unresolved = analysis
        .references
        .iter()
        .find(|reference| {
            reference.source == widget.id && reference.kind == RelationshipKind::Implements
        })
        .expect("trait implementation reference is retained");
    assert_eq!(unresolved.path.display, "ExternalTrait");
}

#[test]
fn extracts_generics_bounds_defaults_and_visibility() {
    let analysis = analyze_fixture("generics");
    assert!(analysis.diagnostics.is_empty());

    let container = type_named(&analysis, "Container");
    assert_eq!(container.generics.len(), 2);
    let lifetime = container
        .generics
        .iter()
        .find(|generic| generic.name == "'a")
        .expect("lifetime generic");
    assert!(lifetime.bounds.is_empty());
    let value_type = container
        .generics
        .iter()
        .find(|generic| generic.name == "T")
        .expect("type generic");
    assert_eq!(value_type.bounds, vec!["Clone", "Send", "Default"]);
    assert_eq!(value_type.default.as_deref(), Some("String"));
    assert_eq!(container.fields[0].name, "value");
    assert_eq!(container.fields[0].type_name.as_deref(), Some("T"));
    assert_eq!(container.fields[1].type_name.as_deref(), Some("&'a str"));
    assert_eq!(container.methods.len(), 1);
    assert_eq!(container.methods[0].name, "value");
    assert_eq!(container.methods[0].return_type.as_deref(), Some("&T"));

    assert_eq!(
        type_named(&analysis, "CrateVisible").visibility,
        Visibility::Crate
    );
    assert_eq!(
        type_named(&analysis, "SuperVisible").visibility,
        Visibility::Super
    );
    match &type_named(&analysis, "Restricted").visibility {
        Visibility::Restricted(path) => assert!(path.contains("crate::nested")),
        other => panic!("expected restricted visibility, got {other:?}"),
    }
}

#[test]
fn resolves_inline_and_external_modules_with_conventional_paths() {
    let analysis = analyze_fixture("modules");
    assert_eq!(analysis.files_failed, 0);
    assert!(analysis.diagnostics.is_empty());

    let root = module_qualified(&analysis, "modules");
    let external = module_qualified(&analysis, "modules::external");
    let nested = module_qualified(&analysis, "modules::external::nested");
    let inner = module_qualified(&analysis, "modules::inline_outer::inline_inner");
    let directory = module_qualified(&analysis, "modules::directory");
    let child = module_qualified(&analysis, "modules::directory::child");

    assert_eq!(root.file, PathBuf::from("src/lib.rs"));
    assert_eq!(external.file, PathBuf::from("src/external.rs"));
    assert_eq!(nested.file, PathBuf::from("src/external/nested.rs"));
    assert_eq!(inner.file, PathBuf::from("src/lib.rs"));
    assert_eq!(inner.location.line, 7);
    assert_eq!(directory.file, PathBuf::from("src/directory/mod.rs"));
    assert_eq!(child.file, PathBuf::from("src/directory/child.rs"));

    assert_eq!(
        type_named(&analysis, "RootType").module.as_ref(),
        Some(&root.id)
    );
    assert_eq!(
        type_named(&analysis, "External").module.as_ref(),
        Some(&external.id)
    );
    assert_eq!(
        type_named(&analysis, "Nested").module.as_ref(),
        Some(&nested.id)
    );
    assert_eq!(type_named(&analysis, "Deep").location.line, 8);
    assert_eq!(
        type_named(&analysis, "Deep").module.as_ref(),
        Some(&inner.id)
    );
    assert_eq!(
        type_named(&analysis, "Directory").module.as_ref(),
        Some(&directory.id)
    );
    assert_eq!(
        type_named(&analysis, "Child").module.as_ref(),
        Some(&child.id)
    );
}

#[test]
fn unresolved_external_modules_are_reported() {
    let analysis = analyze_fixture("unresolved_module");

    assert!(type_named(&analysis, "Root")
        .id
        .as_str()
        .contains("src/lib.rs"));
    let diagnostic = analysis
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_deref() == Some("rust::unresolved-module"))
        .expect("unresolved module diagnostic");
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
    assert!(diagnostic.message.contains("absent"));
    assert_eq!(
        diagnostic.location.as_ref().unwrap().file,
        PathBuf::from("src/lib.rs")
    );
    assert!(!analysis
        .modules
        .iter()
        .any(|module| module.qualified_name == "unresolved_module::absent"));
}

#[test]
fn distinguishes_duplicate_short_names_and_is_deterministic() {
    let first = analyze_fixture("modules");
    let second = analyze_fixture("modules");

    let first_same: Vec<_> = first
        .types
        .iter()
        .filter(|declaration| declaration.name == "SameName")
        .collect();
    let second_same: Vec<_> = second
        .types
        .iter()
        .filter(|declaration| declaration.name == "SameName")
        .collect();
    assert_eq!(first_same.len(), 2);
    assert_eq!(second_same.len(), 2);
    assert_ne!(first_same[0].id, first_same[1].id);
    assert_ne!(first_same[0].module, first_same[1].module);
    let id_texts: Vec<_> = first_same
        .iter()
        .map(|declaration| declaration.id.as_str())
        .collect();
    assert!(id_texts.iter().any(|id| id.contains("src/other.rs")));
    assert!(id_texts.iter().any(|id| id.contains("src/second.rs")));

    let first_type_ids: Vec<_> = first
        .types
        .iter()
        .map(|declaration| &declaration.id)
        .collect();
    let second_type_ids: Vec<_> = second
        .types
        .iter()
        .map(|declaration| &declaration.id)
        .collect();
    assert_eq!(first_type_ids, second_type_ids);

    let first_module_ids: Vec<_> = first.modules.iter().map(|module| &module.id).collect();
    let second_module_ids: Vec<_> = second.modules.iter().map(|module| &module.id).collect();
    assert_eq!(first_module_ids, second_module_ids);
}

#[test]
fn preserves_valid_results_when_one_file_fails_to_parse() {
    let analysis = analyze_fixture("malformed");

    assert_eq!(analysis.files_analyzed, 2);
    assert_eq!(analysis.files_failed, 1);
    assert!(type_named(&analysis, "Root")
        .id
        .as_str()
        .contains("src/lib.rs"));
    assert!(type_named(&analysis, "Good")
        .id
        .as_str()
        .contains("src/good.rs"));
    assert!(!analysis
        .types
        .iter()
        .any(|declaration| declaration.name == "Broken"));

    let parse_diagnostic = analysis
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Error
                && diagnostic
                    .location
                    .as_ref()
                    .is_some_and(|location| location.file == Path::new("src/broken.rs"))
        })
        .expect("parse error diagnostic with file location");
    assert!(parse_diagnostic.message.to_lowercase().contains("parse"));
    assert_eq!(parse_diagnostic.code.as_deref(), Some("rust::parse-error"));
    assert_eq!(
        parse_diagnostic.location.as_ref().unwrap().line,
        1,
        "syntax error should report its real line"
    );
}

#[test]
fn a_valid_file_without_declarations_is_not_a_parse_failure() {
    let analysis = analyze_fixture("empty");

    assert_eq!(analysis.files_analyzed, 1);
    assert_eq!(analysis.files_failed, 0);
    assert!(analysis.types.is_empty());
    assert!(analysis.diagnostics.is_empty());
}

#[test]
fn binary_crate_roots_use_their_target_name_in_stable_ids() {
    let analysis = analyze_fixture_with_target("binary", "app", "src/main.rs", false);
    assert_eq!(analysis.files_failed, 0);

    let root = module_qualified(&analysis, "binary::app");
    assert_eq!(root.file, PathBuf::from("src/main.rs"));
    assert_eq!(
        type_named(&analysis, "Helper").module.as_ref(),
        Some(&module_qualified(&analysis, "binary::app::helper").id)
    );
    assert_eq!(
        type_named(&analysis, "Helper").qualified_name,
        "binary::app::helper::Helper"
    );
}

#[test]
fn impls_for_ambiguous_types_are_reported_without_attaching_methods() {
    let analysis = analyze_fixture("references");
    let diagnostic = analysis
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_deref() == Some("rust::ambiguous-impl-target"))
        .expect("ambiguous impl target diagnostic");
    assert!(diagnostic.message.contains("Duplicated"));
    assert!(!analysis.types.iter().any(|declaration| declaration
        .methods
        .iter()
        .any(|method| method.name == "skipped")));
}
