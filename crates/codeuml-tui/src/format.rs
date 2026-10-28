use codeuml_core::{Codebase, Diagnostic, GenericParam, TypeDecl};
use codeuml_diagram::{DiagramEndpoint, DiagramProjection};

pub(crate) fn detail_lines(codebase: &Codebase, declaration: &TypeDecl) -> Vec<String> {
    let mut lines = header_lines(codebase, declaration);
    append_generics(&mut lines, declaration);
    append_fields(&mut lines, declaration);
    append_variants(&mut lines, declaration);
    append_methods(&mut lines, declaration);
    lines
}

fn header_lines(codebase: &Codebase, declaration: &TypeDecl) -> Vec<String> {
    let mut lines = vec![
        format!("Name: {}", declaration.name),
        format!("Kind: {}", declaration.kind),
        format!("Qualified: {}", declaration.qualified_name),
        format!("Visibility: {}", declaration.visibility),
        format!(
            "Source: {}:{}:{}",
            declaration.location.file.display(),
            declaration.location.line,
            declaration.location.column
        ),
    ];
    if let Some(module_id) = &declaration.module {
        if let Some(module) = codebase.module(module_id) {
            lines.push(format!("Module: {}", module.qualified_name));
        }
    }
    if let Some(alias) = &declaration.alias_of {
        lines.push(format!("Alias of: {alias}"));
    }
    lines
}

fn append_generics(lines: &mut Vec<String>, declaration: &TypeDecl) {
    if declaration.generics.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("Generics ({})", declaration.generics.len()));
    for generic in &declaration.generics {
        lines.push(format!("  {}", generic_text(generic)));
    }
}

fn append_fields(lines: &mut Vec<String>, declaration: &TypeDecl) {
    if declaration.fields.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("Fields ({})", declaration.fields.len()));
    for field in &declaration.fields {
        let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
        lines.push(format!(
            "  {} {}: {}  (line {})",
            field.visibility, field.name, type_name, field.location.line
        ));
    }
}

fn append_variants(lines: &mut Vec<String>, declaration: &TypeDecl) {
    if declaration.variants.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("Variants ({})", declaration.variants.len()));
    for variant in &declaration.variants {
        lines.push(format!(
            "  {}  (line {})",
            variant.name, variant.location.line
        ));
        for field in &variant.fields {
            let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
            lines.push(format!("    {}: {}", field.name, type_name));
        }
    }
}

fn append_methods(lines: &mut Vec<String>, declaration: &TypeDecl) {
    if declaration.methods.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("Methods ({})", declaration.methods.len()));
    for method in &declaration.methods {
        let parameters = method
            .parameters
            .iter()
            .map(|parameter| format!("{}: {}", parameter.name, parameter.type_name))
            .collect::<Vec<_>>()
            .join(", ");
        let generic_text = method_generics(method.generics.as_slice());
        let return_text = method
            .return_type
            .as_ref()
            .map(|return_type| format!(" -> {return_type}"))
            .unwrap_or_default();
        lines.push(format!(
            "  {} {}{}{}({}){}  (line {})",
            method.visibility,
            if method.is_static { "static " } else { "" },
            method.name,
            generic_text,
            parameters,
            return_text,
            method.location.line
        ));
    }
}

fn method_generics(generics: &[GenericParam]) -> String {
    if generics.is_empty() {
        return String::new();
    }
    format!(
        "<{}>",
        generics
            .iter()
            .map(generic_text)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn generic_text(generic: &GenericParam) -> String {
    let mut text = generic.name.clone();
    if !generic.bounds.is_empty() {
        text.push_str(": ");
        text.push_str(&generic.bounds.join(" + "));
    }
    if let Some(default) = &generic.default {
        text.push_str(" = ");
        text.push_str(default);
    }
    text
}

pub(crate) fn endpoint_label(projection: &DiagramProjection, endpoint: &DiagramEndpoint) -> String {
    match endpoint {
        DiagramEndpoint::Node(id) => projection
            .nodes
            .iter()
            .find(|node| &node.id == id)
            .map(|node| node.qualified_name.clone())
            .unwrap_or_else(|| id.as_str().to_owned()),
        DiagramEndpoint::Boundary { name, .. } => name.clone(),
    }
}

pub(crate) fn diagnostic_line(diagnostic: &Diagnostic) -> String {
    let location = diagnostic
        .location
        .as_ref()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file.display(),
                location.line,
                location.column
            )
        })
        .unwrap_or_else(|| "<no location>".to_owned());
    let code = diagnostic
        .code
        .as_ref()
        .map(|code| format!(" [{code}]"))
        .unwrap_or_default();
    format!(
        "[{}] {location} {}{}",
        diagnostic.severity, diagnostic.message, code
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{fixture_codebase, type_named};

    #[test]
    fn detail_sections_cover_generics_fields_variants_methods_and_empty_sections() {
        let codebase = fixture_codebase();
        let alpha = detail_lines(&codebase, type_named(&codebase, "Alpha")).join("\n");
        for section in [
            "Module: models",
            "Generics (1)",
            "T: Clone",
            "Fields (2)",
            "Methods (2)",
            "find(id: u64) -> Beta",
        ] {
            assert!(alpha.contains(section), "missing {section} in {alpha}");
        }
        assert!(alpha.contains("measure() -> Gamma"));
        let mut static_alias = type_named(&codebase, "Alpha").clone();
        static_alias.alias_of = Some("models::Target".to_owned());
        static_alias.methods[0].is_static = true;
        let alias_text = detail_lines(&codebase, &static_alias).join("\n");
        assert!(alias_text.contains("Alias of: models::Target"));
        assert!(alias_text.contains("pub static find"));

        let shape = detail_lines(&codebase, type_named(&codebase, "Shape")).join("\n");
        assert!(shape.contains("Variants (2)"));
        assert!(shape.contains("Circle  (line 28)"));
        assert!(shape.contains("radius: f64"));

        let api = detail_lines(&codebase, type_named(&codebase, "Api")).join("\n");
        assert!(api.contains("Fields (1)"));
        assert!(!api.contains("Generics ("));
        assert!(!api.contains("Variants ("));
        assert!(!api.contains("Methods ("));
    }
}
