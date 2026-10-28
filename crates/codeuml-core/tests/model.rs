use std::path::Path;

use codeuml_core::{
    AnalysisMetadata, AnalysisMode, Codebase, Diagnostic, DiagnosticSeverity, Field, GenericParam,
    Language, ModelError, Module, Provenance, Relationship, RelationshipEvidence, RelationshipKind,
    RelationshipTarget, ResolutionStatus, SourceFile, SourceLocation, SymbolId, TypeDecl, TypeKind,
    Variant, Visibility,
};

fn location(file: &str, line: u32) -> SourceLocation {
    SourceLocation::new(file, line, 1, line, 8)
}

fn module(id: &str, name: &str, file: &str) -> Module {
    Module::new(
        SymbolId::declaration(file, id),
        name,
        id,
        file,
        location(file, 1),
    )
}

fn type_decl(id: &str, name: &str, kind: TypeKind, file: &str, line: u32) -> TypeDecl {
    TypeDecl::new(
        SymbolId::declaration(file, id),
        name,
        id,
        kind,
        location(file, line),
    )
}

fn populated_codebase() -> (Codebase, SymbolId, SymbolId) {
    let mut codebase = Codebase::new("demo");
    codebase.add_file(SourceFile::new("src/lib.rs", Language::Rust));
    codebase
        .add_module(module("models", "models", "src/models.rs"))
        .expect("module IDs are unique");
    codebase
        .add_module(module("api", "api", "src/api.rs"))
        .expect("module IDs are unique");

    let models_id = SymbolId::declaration("src/models.rs", "models");
    let api_id = SymbolId::declaration("src/api.rs", "api");

    let models_type = type_decl("models::User", "User", TypeKind::Struct, "src/models.rs", 4)
        .with_module(models_id.clone());
    let api_type = type_decl("api::User", "User", TypeKind::Struct, "src/api.rs", 4)
        .with_module(api_id.clone());

    codebase.add_type(models_type).expect("type IDs are unique");
    codebase.add_type(api_type).expect("type IDs are unique");

    (codebase, models_id, api_id)
}

#[test]
fn symbol_ids_are_deterministic_and_disambiguate_modules() {
    let user_in_models = SymbolId::declaration(Path::new("src/models.rs"), "models::User");
    let user_in_api = SymbolId::declaration(Path::new("src/api.rs"), "api::User");

    assert_ne!(user_in_models, user_in_api);
    assert_eq!(
        user_in_models,
        SymbolId::declaration(Path::new("src/models.rs"), "models::User")
    );
    assert_eq!(user_in_models.as_str(), "src/models.rs#models::User");

    let id_field = SymbolId::member(&user_in_models, "id");
    let name_field = SymbolId::member(&user_in_models, "name");
    assert_ne!(id_field, name_field);
    assert_ne!(id_field, user_in_models);
}

#[test]
fn field_method_and_variant_ids_do_not_collide() {
    let parent = SymbolId::declaration(Path::new("src/lib.rs"), "User");

    let field = SymbolId::field(&parent, "value");
    let method = SymbolId::method(&parent, "value");
    let variant = SymbolId::variant(&parent, "value");
    let trait_method = SymbolId::method_in(&parent, "Display", "fmt");

    assert_ne!(field, method);
    assert_ne!(field, variant);
    assert_ne!(method, variant);
    assert_ne!(trait_method, SymbolId::method(&parent, "fmt"));
}

#[test]
fn symbol_id_paths_are_normalized_for_stable_ids() {
    let unix = SymbolId::declaration(Path::new("src/models/user.rs"), "models::user::User");
    let windows = SymbolId::declaration(Path::new(r"src\models\user.rs"), "models::user::User");

    assert_eq!(unix, windows);
}

#[test]
fn identical_short_names_remain_distinct_declarations() {
    let (codebase, models_id, api_id) = populated_codebase();

    let models_users: Vec<_> = codebase.types_in_module(&models_id).collect();
    let api_users: Vec<_> = codebase.types_in_module(&api_id).collect();

    assert_eq!(models_users.len(), 1);
    assert_eq!(api_users.len(), 1);
    assert_eq!(models_users[0].name, "User");
    assert_eq!(api_users[0].name, "User");
    assert_ne!(models_users[0].id, api_users[0].id);
}

#[test]
fn duplicate_declaration_ids_are_rejected() {
    let mut codebase = Codebase::new("demo");
    let decl = type_decl("models::User", "User", TypeKind::Struct, "src/lib.rs", 3);

    codebase
        .add_type(decl.clone())
        .expect("first declaration is accepted");
    let error = codebase
        .add_type(decl.clone())
        .expect_err("duplicate declaration IDs must be rejected");

    assert!(matches!(
        error,
        ModelError::DuplicateDeclaration(id) if id == decl.id
    ));
}

#[test]
fn duplicate_member_ids_are_rejected_within_a_type() {
    let mut codebase = Codebase::new("demo");
    let type_id = SymbolId::declaration("src/lib.rs", "User");
    let field_id = SymbolId::member(&type_id, "id");

    let mut decl = TypeDecl::new(
        type_id,
        "User",
        "User",
        TypeKind::Struct,
        location("src/lib.rs", 1),
    );
    decl.fields.push(Field::new(
        field_id.clone(),
        "id",
        location("src/lib.rs", 2),
    ));
    decl.fields
        .push(Field::new(field_id, "id_again", location("src/lib.rs", 3)));

    let error = codebase
        .add_type(decl)
        .expect_err("member IDs must be unique");

    assert!(matches!(error, ModelError::DuplicateDeclaration(_)));
}

#[test]
fn trait_bounds_are_distinct_from_implementation_relationships() {
    assert_ne!(
        codeuml_core::RelationshipKind::TraitBound,
        codeuml_core::RelationshipKind::Implements
    );
    assert_eq!(
        codeuml_core::RelationshipKind::TraitBound.to_string(),
        "trait bound"
    );
}

#[test]
fn relationships_are_deduplicated_by_source_target_and_kind() {
    let (mut codebase, models_id, _) = populated_codebase();
    let user = SymbolId::declaration("src/models.rs", "models::User");
    let trait_id = SymbolId::declaration("src/models.rs", "models::Entity");
    let mut trait_decl = type_decl(
        "models::Entity",
        "Entity",
        TypeKind::Trait,
        "src/models.rs",
        10,
    )
    .with_module(models_id);
    trait_decl.methods.push(codeuml_core::Method::new(
        SymbolId::member(&trait_id, "id"),
        "id",
        location("src/models.rs", 11),
    ));
    codebase
        .add_type(trait_decl)
        .expect("trait declaration is accepted");

    let first = Relationship::inferred(&user, &trait_id, RelationshipKind::Implements)
        .with_evidence(RelationshipEvidence::new(location("src/models.rs", 20)));
    let duplicate = Relationship::inferred(&user, &trait_id, RelationshipKind::Implements)
        .with_evidence(RelationshipEvidence::new(location("src/models.rs", 21)));

    assert!(codebase
        .add_relationship(first)
        .expect("known declarations are accepted"));
    assert!(!codebase
        .add_relationship(duplicate)
        .expect("known declarations are accepted"));

    assert_eq!(codebase.relationships().len(), 1);
    assert_eq!(codebase.relationships()[0].evidence.len(), 2);
}

#[test]
fn strongest_resolution_and_provenance_survive_deduplication() {
    let (mut codebase, _, _) = populated_codebase();
    let user = SymbolId::declaration("src/models.rs", "models::User");
    let entity = SymbolId::declaration("src/models.rs", "models::Entity");
    let mut entity_decl = type_decl(
        "models::Entity",
        "Entity",
        TypeKind::Trait,
        "src/models.rs",
        10,
    );
    entity_decl.id = entity.clone();
    codebase
        .add_type(entity_decl)
        .expect("trait declaration is accepted");

    codebase
        .add_relationship(Relationship::inferred(
            &user,
            &entity,
            RelationshipKind::Implements,
        ))
        .expect("inferred relationship is accepted");
    codebase
        .add_relationship(
            Relationship::resolved(&user, &entity, RelationshipKind::Implements)
                .with_evidence(RelationshipEvidence::new(location("src/models.rs", 20))),
        )
        .expect("resolved relationship is accepted");

    let relationship = &codebase.relationships()[0];
    assert_eq!(relationship.resolution, ResolutionStatus::Resolved);
    assert_eq!(relationship.provenance, Provenance::Semantic);
}

#[test]
fn unresolved_relationships_are_preserved_without_a_declaration() {
    let (mut codebase, _, _) = populated_codebase();
    let user = SymbolId::declaration("src/models.rs", "models::User");
    let evidence = RelationshipEvidence::new(location("src/models.rs", 12));
    let relationship =
        Relationship::unresolved(&user, "external::Repository", RelationshipKind::FieldType)
            .with_evidence(evidence.clone());

    codebase
        .add_relationship(relationship)
        .expect("unresolved targets are allowed");

    let stored = &codebase.relationships()[0];
    assert_eq!(stored.resolution, ResolutionStatus::Unresolved);
    assert_eq!(
        stored.target,
        RelationshipTarget::Unresolved(codeuml_core::UnresolvedReference::new(
            "external::Repository"
        ))
    );
    assert_eq!(stored.evidence, vec![evidence]);
}

#[test]
fn resolved_relationships_require_existing_declarations() {
    let (mut codebase, _, _) = populated_codebase();
    let user = SymbolId::declaration("src/models.rs", "models::User");
    let missing = SymbolId::declaration("src/models.rs", "models::Missing");

    let error = codebase
        .add_relationship(Relationship::inferred(
            &user,
            &missing,
            RelationshipKind::FieldType,
        ))
        .expect_err("targets must exist");

    assert!(matches!(
        error,
        ModelError::UnknownRelationshipTarget(id) if id == missing
    ));
}

#[test]
fn relationship_target_and_resolution_must_agree() {
    let (mut codebase, _, _) = populated_codebase();
    let user = SymbolId::declaration("src/models.rs", "models::User");

    let mut relationship = Relationship::inferred(
        &user,
        &SymbolId::declaration("src/models.rs", "models::Missing"),
        RelationshipKind::FieldType,
    );
    relationship.resolution = ResolutionStatus::Unresolved;

    let error = codebase
        .add_relationship(relationship)
        .expect_err("an unresolved status requires an unresolved target");

    assert!(matches!(error, ModelError::InconsistentRelationship { .. }));
}

#[test]
fn type_declarations_keep_members_generics_and_visibility() {
    let mut codebase = Codebase::new("demo");
    let type_id = SymbolId::declaration("src/lib.rs", "User");
    let mut decl = TypeDecl::new(
        type_id.clone(),
        "User",
        "User",
        TypeKind::Struct,
        location("src/lib.rs", 1),
    );
    decl.visibility = Visibility::Public;
    decl.generics
        .push(GenericParam::new("T").with_bounds(["Clone", "Send"]));
    decl.fields.push(
        Field::new(
            SymbolId::member(&type_id, "id"),
            "id",
            location("src/lib.rs", 2),
        )
        .with_type("u64")
        .with_visibility(Visibility::Private),
    );
    decl.methods.push(
        codeuml_core::Method::new(
            SymbolId::member(&type_id, "new"),
            "new",
            location("src/lib.rs", 4),
        )
        .with_return_type("Self")
        .with_visibility(Visibility::Public),
    );

    codebase.add_type(decl).expect("declaration is accepted");

    let stored = &codebase.types()[0];
    assert_eq!(stored.visibility, Visibility::Public);
    assert_eq!(stored.generics[0].bounds, vec!["Clone", "Send"]);
    assert_eq!(stored.fields[0].type_name.as_deref(), Some("u64"));
    assert_eq!(stored.methods[0].return_type.as_deref(), Some("Self"));
}

#[test]
fn enums_preserve_variants_and_variant_fields() {
    let mut codebase = Codebase::new("demo");
    let enum_id = SymbolId::declaration("src/lib.rs", "Status");
    let mut decl = TypeDecl::new(
        enum_id.clone(),
        "Status",
        "Status",
        TypeKind::Enum,
        location("src/lib.rs", 1),
    );
    let mut variant = Variant::new(
        SymbolId::member(&enum_id, "Active"),
        "Active",
        location("src/lib.rs", 2),
    );
    variant.fields.push(Field::new(
        SymbolId::member(&variant.id, "0"),
        "0",
        location("src/lib.rs", 2),
    ));
    decl.variants.push(variant);

    codebase.add_type(decl).expect("declaration is accepted");

    assert_eq!(codebase.types()[0].variants[0].name, "Active");
    assert_eq!(codebase.types()[0].variants[0].fields.len(), 1);
}

#[test]
fn diagnostics_are_structured_and_optional_location() {
    let mut codebase = Codebase::new("demo");
    codebase.add_diagnostic(Diagnostic::error(
        "failed to parse file",
        Some(location("src/broken.rs", 7)),
    ));
    codebase.add_diagnostic(Diagnostic::warning("no source files found", None));

    assert_eq!(codebase.diagnostics().len(), 2);
    assert_eq!(
        codebase.diagnostics()[0].severity,
        DiagnosticSeverity::Error
    );
    assert_eq!(codebase.diagnostics()[0].location.as_ref().unwrap().line, 7);
    assert!(codebase.diagnostics()[1].location.is_none());
}

#[test]
fn analysis_metadata_defaults_to_syntax_only() {
    let metadata = AnalysisMetadata::default();

    assert_eq!(metadata.mode, AnalysisMode::SyntaxOnly);
    assert!(!metadata.partial);
    assert_eq!(metadata.files_analyzed, 0);
    assert_eq!(metadata.files_failed, 0);
}

#[test]
fn files_are_not_duplicated_by_path() {
    let mut codebase = Codebase::new("demo");

    assert!(codebase.add_file(SourceFile::new("src/lib.rs", Language::Rust)));
    assert!(!codebase.add_file(SourceFile::new("src/lib.rs", Language::Rust)));

    assert_eq!(codebase.files().len(), 1);
}
