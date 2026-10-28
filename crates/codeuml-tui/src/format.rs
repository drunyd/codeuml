use codeuml_core::{Codebase, Diagnostic, GenericParam, TypeDecl};
use codeuml_diagram::{DiagramEndpoint, DiagramProjection};

pub(crate) fn detail_lines(codebase: &Codebase, declaration: &TypeDecl) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("Name: {}", declaration.name));
    lines.push(format!("Kind: {}", declaration.kind));
    lines.push(format!("Qualified: {}", declaration.qualified_name));
    lines.push(format!("Visibility: {}", declaration.visibility));
    lines.push(format!(
        "Source: {}:{}:{}",
        declaration.location.file.display(),
        declaration.location.line,
        declaration.location.column
    ));
    if let Some(module_id) = &declaration.module {
        if let Some(module) = codebase.module(module_id) {
            lines.push(format!("Module: {}", module.qualified_name));
        }
    }
    if let Some(alias) = &declaration.alias_of {
        lines.push(format!("Alias of: {alias}"));
    }

    if !declaration.generics.is_empty() {
        lines.push(String::new());
        lines.push(format!("Generics ({})", declaration.generics.len()));
        for generic in &declaration.generics {
            lines.push(format!("  {}", generic_text(generic)));
        }
    }

    if !declaration.fields.is_empty() {
        lines.push(String::new());
        lines.push(format!("Fields ({})", declaration.fields.len()));
        for field in &declaration.fields {
            let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
            lines.push(format!(
                "  {} {}: {}",
                field.visibility, field.name, type_name
            ));
        }
    }

    if !declaration.variants.is_empty() {
        lines.push(String::new());
        lines.push(format!("Variants ({})", declaration.variants.len()));
        for variant in &declaration.variants {
            lines.push(format!("  {}", variant.name));
            for field in &variant.fields {
                let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
                lines.push(format!("    {}: {}", field.name, type_name));
            }
        }
    }

    if !declaration.methods.is_empty() {
        lines.push(String::new());
        lines.push(format!("Methods ({})", declaration.methods.len()));
        for method in &declaration.methods {
            let parameters = method
                .parameters
                .iter()
                .map(|parameter| format!("{}: {}", parameter.name, parameter.type_name))
                .collect::<Vec<_>>()
                .join(", ");
            let generic_text = if method.generics.is_empty() {
                String::new()
            } else {
                format!(
                    "<{}>",
                    method
                        .generics
                        .iter()
                        .map(generic_text)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let return_text = method
                .return_type
                .as_ref()
                .map(|return_type| format!(" -> {return_type}"))
                .unwrap_or_default();
            lines.push(format!(
                "  {} {}{}{}({}){}",
                method.visibility,
                if method.is_static { "static " } else { "" },
                method.name,
                generic_text,
                parameters,
                return_text
            ));
        }
    }

    lines
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
