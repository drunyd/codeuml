use std::path::PathBuf;

use codeuml_core::{
    Codebase, Diagnostic, Field, GenericParam, Method, Module, Parameter, Relationship,
    RelationshipEvidence, RelationshipKind, SourceLocation, SymbolId, TypeDecl, TypeKind, Variant,
    Visibility,
};

pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codeuml-tui-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Source files matching the locations used by `fixture_codebase`.
pub(crate) fn fixture_project(name: &str) -> TempDir {
    let dir = TempDir::new(name);
    std::fs::create_dir_all(dir.0.join("src")).expect("src dir");
    for file in ["models", "api"] {
        let text: String = (1..=40)
            .map(|line| format!("// {file} line {line}\n"))
            .collect();
        std::fs::write(dir.0.join(format!("src/{file}.rs")), text).expect("fixture source");
    }
    dir
}

pub(crate) fn location(file: &str, line: u32) -> SourceLocation {
    SourceLocation::new(file, line, 1, line, 20)
}

pub(crate) fn fixture_codebase() -> Codebase {
    let mut codebase = Codebase::new("fixture-project");
    let models = SymbolId::declaration("src/models.rs", "models");
    let api = SymbolId::declaration("src/api.rs", "api");
    codebase
        .add_module(Module::new(
            models.clone(),
            "models",
            "models",
            "src/models.rs",
            location("src/models.rs", 1),
        ))
        .expect("unique module");
    codebase
        .add_module(Module::new(
            api.clone(),
            "api",
            "api",
            "src/api.rs",
            location("src/api.rs", 1),
        ))
        .expect("unique module");

    let alpha = SymbolId::declaration("src/models.rs", "models::Alpha");
    let mut alpha_decl = TypeDecl::new(
        alpha.clone(),
        "Alpha",
        "models::Alpha",
        TypeKind::Struct,
        location("src/models.rs", 3),
    )
    .with_module(models.clone())
    .with_visibility(Visibility::Public)
    .with_generics([GenericParam::new("T").with_bounds(["Clone"])]);
    let beta_field = SymbolId::field(&alpha, "beta");
    alpha_decl.fields.push(
        Field::new(beta_field.clone(), "beta", location("src/models.rs", 4))
            .with_type("Beta")
            .with_visibility(Visibility::Public),
    );
    alpha_decl.fields.push(
        Field::new(
            SymbolId::field(&alpha, "external"),
            "external",
            location("src/models.rs", 5),
        )
        .with_type("ExternalType")
        .with_visibility(Visibility::Private),
    );
    let find = SymbolId::method(&alpha, "find");
    alpha_decl.methods.push(
        Method::new(find.clone(), "find", location("src/models.rs", 8))
            .with_parameters([Parameter::new("id", "u64")])
            .with_return_type("Beta")
            .with_visibility(Visibility::Public),
    );
    let measure = SymbolId::method(&alpha, "measure");
    alpha_decl.methods.push(
        Method::new(measure.clone(), "measure", location("src/models.rs", 12))
            .with_return_type("Gamma")
            .with_visibility(Visibility::Private),
    );
    codebase.add_type(alpha_decl).expect("unique type");

    let api_type = SymbolId::declaration("src/api.rs", "api::Api");
    let mut api_decl = TypeDecl::new(
        api_type.clone(),
        "Api",
        "api::Api",
        TypeKind::Struct,
        location("src/api.rs", 3),
    )
    .with_module(api.clone())
    .with_visibility(Visibility::Public);
    let api_alpha_field = SymbolId::field(&api_type, "alpha");
    api_decl.fields.push(
        Field::new(api_alpha_field.clone(), "alpha", location("src/api.rs", 4))
            .with_type("Alpha")
            .with_visibility(Visibility::Public),
    );
    codebase.add_type(api_decl).expect("unique type");

    let beta = SymbolId::declaration("src/models.rs", "models::Beta");
    let mut beta_decl = TypeDecl::new(
        beta.clone(),
        "Beta",
        "models::Beta",
        TypeKind::Struct,
        location("src/models.rs", 16),
    )
    .with_module(models.clone())
    .with_visibility(Visibility::Public);
    beta_decl.fields.push(
        Field::new(
            SymbolId::field(&beta, "name"),
            "name",
            location("src/models.rs", 17),
        )
        .with_type("String")
        .with_visibility(Visibility::Public),
    );
    codebase.add_type(beta_decl).expect("unique type");

    let gamma = SymbolId::declaration("src/models.rs", "models::Gamma");
    let mut gamma_decl = TypeDecl::new(
        gamma.clone(),
        "Gamma",
        "models::Gamma",
        TypeKind::Trait,
        location("src/models.rs", 21),
    )
    .with_module(models.clone())
    .with_visibility(Visibility::Public);
    gamma_decl.methods.push(
        Method::new(
            SymbolId::method(&gamma, "run"),
            "run",
            location("src/models.rs", 22),
        )
        .with_return_type("u64")
        .with_visibility(Visibility::Public),
    );
    codebase.add_type(gamma_decl).expect("unique type");

    let shape = SymbolId::declaration("src/models.rs", "models::Shape");
    let mut shape_decl = TypeDecl::new(
        shape.clone(),
        "Shape",
        "models::Shape",
        TypeKind::Enum,
        location("src/models.rs", 26),
    )
    .with_module(models.clone())
    .with_visibility(Visibility::Public);
    shape_decl.variants.push(Variant::new(
        SymbolId::variant(&shape, "Empty"),
        "Empty",
        location("src/models.rs", 27),
    ));
    let circle = SymbolId::variant(&shape, "Circle");
    let mut circle_variant = Variant::new(circle.clone(), "Circle", location("src/models.rs", 28));
    circle_variant.fields.push(
        Field::new(
            SymbolId::field(&circle, "radius"),
            "radius",
            location("src/models.rs", 28),
        )
        .with_type("f64"),
    );
    shape_decl.variants.push(circle_variant);
    codebase.add_type(shape_decl).expect("unique type");

    codebase
        .add_relationship(
            Relationship::inferred(&beta_field, &beta, RelationshipKind::FieldType)
                .with_evidence(RelationshipEvidence::new(location("src/models.rs", 4))),
        )
        .expect("valid relationship");
    codebase
        .add_relationship(
            Relationship::inferred(&find, &beta, RelationshipKind::MethodReturnType)
                .with_evidence(RelationshipEvidence::new(location("src/models.rs", 8))),
        )
        .expect("valid relationship");
    codebase
        .add_relationship(
            Relationship::inferred(&measure, &gamma, RelationshipKind::MethodReturnType)
                .with_evidence(RelationshipEvidence::new(location("src/models.rs", 12))),
        )
        .expect("valid relationship");
    codebase
        .add_relationship(
            Relationship::unresolved(
                &SymbolId::field(&alpha, "external"),
                "ExternalType",
                RelationshipKind::FieldType,
            )
            .with_evidence(RelationshipEvidence::new(location("src/models.rs", 5))),
        )
        .expect("valid relationship");
    codebase
        .add_relationship(
            Relationship::inferred(&api_alpha_field, &alpha, RelationshipKind::FieldType)
                .with_evidence(RelationshipEvidence::new(location("src/api.rs", 4))),
        )
        .expect("valid relationship");

    codebase.add_diagnostic(
        Diagnostic::warning(
            "ambiguous reference from fixture",
            Some(location("src/models.rs", 5)),
        )
        .with_code("fixture::warning"),
    );
    codebase.add_diagnostic(Diagnostic::error("fixture file failed to parse", None));
    codebase
}

pub(crate) fn type_named<'a>(codebase: &'a Codebase, name: &str) -> &'a TypeDecl {
    codebase
        .types()
        .iter()
        .find(|declaration| declaration.name == name)
        .unwrap_or_else(|| panic!("missing fixture type {name}"))
}
