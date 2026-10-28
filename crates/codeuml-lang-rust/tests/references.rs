use std::fs;
use std::path::{Path, PathBuf};

use codeuml_core::{RelationshipKind, TypeDecl};
use codeuml_lang_rust::{
    analyze_package, RustAnalysis, RustImport, RustPackageSources, RustPathRoot, RustReference,
    RustTargetSource, RustTypePath,
};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/references")
}

fn collect_rust_sources(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read fixture directory") {
            let path = entry.expect("read fixture entry").path();
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path.strip_prefix(root).expect("relative path");
                files.push(PathBuf::from(relative.to_string_lossy().replace('\\', "/")));
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files.sort();
    files
}

fn analyze() -> RustAnalysis {
    let root = fixture_root();
    analyze_package(
        &root,
        &RustPackageSources {
            name: "references".to_owned(),
            source_files: collect_rust_sources(&root),
            targets: vec![RustTargetSource {
                name: "references".to_owned(),
                source_path: PathBuf::from("src/lib.rs"),
                is_library: true,
            }],
        },
    )
}

fn type_named<'a>(analysis: &'a RustAnalysis, name: &str) -> &'a TypeDecl {
    analysis
        .types
        .iter()
        .find(|declaration| declaration.name == name)
        .unwrap_or_else(|| panic!("missing type {name}"))
}

fn references_for_source<'a>(
    analysis: &'a RustAnalysis,
    source: &codeuml_core::SymbolId,
    kind: RelationshipKind,
) -> Vec<&'a RustReference> {
    analysis
        .references
        .iter()
        .filter(|reference| &reference.source == source && reference.kind == kind)
        .collect()
}

fn path_with_display<'a>(references: &'a [&'a RustReference], display: &str) -> &'a RustTypePath {
    references
        .iter()
        .map(|reference| &reference.path)
        .find(|path| path.display == display)
        .unwrap_or_else(|| panic!("missing reference path {display}"))
}

fn named_import<'a>(imports: &'a [RustImport], alias: &str) -> &'a RustTypePath {
    imports
        .iter()
        .find_map(|import| match import {
            RustImport::Named {
                alias: import_alias,
                path,
            } if import_alias == alias => Some(path),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing named import {alias}"))
}

#[test]
fn extracts_field_parameter_return_alias_and_bound_references_with_context() {
    let analysis = analyze();
    assert_eq!(analysis.files_failed, 0);

    let fields = type_named(&analysis, "Fields");
    let customer_field = fields
        .fields
        .iter()
        .find(|field| field.name == "customer")
        .expect("customer field");
    let field_refs =
        references_for_source(&analysis, &customer_field.id, RelationshipKind::FieldType);
    let customer = path_with_display(&field_refs, "Customer");
    assert_eq!(customer.root, RustPathRoot::Relative);
    assert_eq!(customer.segments, vec!["Customer"]);
    assert_eq!(customer_field.location.line, field_refs[0].location.line);
    assert_eq!(field_refs[0].crate_key, "references");
    assert_eq!(field_refs[0].module_path, Vec::<String>::new());

    let imports = &field_refs[0].imports;
    let purchase_import = named_import(imports, "Purchase");
    assert_eq!(purchase_import.root, RustPathRoot::Crate);
    assert_eq!(purchase_import.segments, vec!["models", "Order"]);
    assert!(imports.iter().any(|import| {
        matches!(import, RustImport::Glob { path } if path.root == RustPathRoot::Crate && path.segments == vec!["other"])
    }));

    let purchase_refs = references_for_source(
        &analysis,
        &fields
            .fields
            .iter()
            .find(|field| field.name == "purchase")
            .unwrap()
            .id,
        RelationshipKind::FieldType,
    );
    assert_eq!(
        path_with_display(&purchase_refs, "Purchase").segments,
        vec!["Purchase"]
    );

    let explicit_refs = references_for_source(
        &analysis,
        &fields
            .fields
            .iter()
            .find(|field| field.name == "explicit")
            .unwrap()
            .id,
        RelationshipKind::FieldType,
    );
    let explicit = path_with_display(&explicit_refs, "crate::models::Customer");
    assert_eq!(explicit.root, RustPathRoot::Crate);
    assert_eq!(explicit.segments, vec!["models", "Customer"]);

    let generic_refs = references_for_source(
        &analysis,
        &fields
            .fields
            .iter()
            .find(|field| field.name == "nested")
            .unwrap()
            .id,
        RelationshipKind::FieldType,
    );
    for display in ["Option", "Box", "Customer"] {
        path_with_display(&generic_refs, display);
    }

    let qualified_refs = references_for_source(
        &analysis,
        &fields
            .fields
            .iter()
            .find(|field| field.name == "qualified")
            .unwrap()
            .id,
        RelationshipKind::FieldType,
    );
    assert!(qualified_refs.iter().any(|reference| {
        reference.path.root == RustPathRoot::Unresolved
            && reference.path.display.contains("<T as Marker>::Item")
    }));

    let value_field = fields
        .fields
        .iter()
        .find(|field| field.name == "value")
        .unwrap();
    let value_refs = references_for_source(&analysis, &value_field.id, RelationshipKind::FieldType);
    let generic_value = value_refs
        .iter()
        .find(|reference| reference.path.display == "T")
        .expect("generic path is retained with context");
    assert!(generic_value.generic_parameters.contains(&"T".to_owned()));

    let alias = type_named(&analysis, "Alias");
    let alias_refs = references_for_source(&analysis, &alias.id, RelationshipKind::Aliases);
    assert_eq!(
        path_with_display(&alias_refs, "Customer").segments,
        vec!["Customer"]
    );
    let deep = type_named(&analysis, "DeepAlias");
    let deep_refs = references_for_source(&analysis, &deep.id, RelationshipKind::Aliases);
    path_with_display(&deep_refs, "Alias");

    let where_bound = type_named(&analysis, "WhereBound");
    let bound_refs =
        references_for_source(&analysis, &where_bound.id, RelationshipKind::TraitBound);
    path_with_display(&bound_refs, "Marker");

    let repo = type_named(&analysis, "Repo");
    let repo_bounds = references_for_source(&analysis, &repo.id, RelationshipKind::TraitBound);
    path_with_display(&repo_bounds, "Marker");

    let convert = fields
        .methods
        .iter()
        .find(|method| method.name == "convert")
        .expect("convert method");
    let parameter_refs = references_for_source(
        &analysis,
        &convert.id,
        RelationshipKind::MethodParameterType,
    );
    path_with_display(&parameter_refs, "Purchase");
    let return_refs =
        references_for_source(&analysis, &convert.id, RelationshipKind::MethodReturnType);
    path_with_display(&return_refs, "Customer");
    let impl_bounds = references_for_source(&analysis, &fields.id, RelationshipKind::TraitBound);
    path_with_display(&impl_bounds, "Marker");
}

#[test]
fn extraction_is_deterministic_and_retains_compound_types() {
    let first = analyze();
    let second = analyze();

    let first_paths: Vec<_> = first
        .references
        .iter()
        .map(|reference| (&reference.source, reference.kind, &reference.path))
        .collect();
    let second_paths: Vec<_> = second
        .references
        .iter()
        .map(|reference| (&reference.source, reference.kind, &reference.path))
        .collect();
    assert_eq!(first_paths, second_paths);

    let fields = type_named(&first, "Fields");
    for field_name in [
        "reference",
        "tuple",
        "array",
        "slice",
        "pointer",
        "callback",
    ] {
        let field = fields
            .fields
            .iter()
            .find(|field| field.name == field_name)
            .unwrap();
        let references = references_for_source(&first, &field.id, RelationshipKind::FieldType);
        assert!(
            references
                .iter()
                .any(|reference| reference.path.display == "Customer"),
            "compound field {field_name} should retain nested Customer reference"
        );
    }
}
