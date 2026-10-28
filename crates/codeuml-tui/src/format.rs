use codeuml_core::{Codebase, Diagnostic, GenericParam, TypeDecl};
use codeuml_diagram::{DiagramEndpoint, DiagramProjection};
use codeuml_metrics::Metrics;

use crate::score::CrapScore;

/// One line of a type's details, optionally carrying a CRAP score to show
/// right-aligned.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DetailRow {
    pub text: String,
    pub crap: Option<CrapScore>,
}

impl DetailRow {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            crap: None,
        }
    }

    fn scored(text: impl Into<String>, crap: Option<CrapScore>) -> Self {
        Self {
            text: text.into(),
            crap,
        }
    }
}

fn score_of(metrics: Option<&Metrics>, value: Option<f64>) -> Option<CrapScore> {
    let metrics = metrics?;
    value.map(|value| CrapScore::new(metrics.level(value), value))
}

pub(crate) fn detail_rows(
    codebase: &Codebase,
    declaration: &TypeDecl,
    metrics: Option<&Metrics>,
) -> Vec<DetailRow> {
    let mut rows = header_rows(codebase, declaration, metrics);
    append_generics(&mut rows, declaration);
    append_fields(&mut rows, declaration);
    append_variants(&mut rows, declaration);
    append_methods(&mut rows, declaration, metrics);
    rows
}

fn header_rows(
    codebase: &Codebase,
    declaration: &TypeDecl,
    metrics: Option<&Metrics>,
) -> Vec<DetailRow> {
    let type_score = score_of(
        metrics,
        metrics.and_then(|metrics| metrics.type_score(&declaration.qualified_name)),
    );
    let mut rows = vec![
        DetailRow::scored(format!("Name: {}", declaration.name), type_score),
        DetailRow::plain(format!("Kind: {}", declaration.kind)),
        DetailRow::plain(format!("Qualified: {}", declaration.qualified_name)),
        DetailRow::plain(format!("Visibility: {}", declaration.visibility)),
        DetailRow::plain(format!(
            "Source: {}:{}:{}",
            declaration.location.file.display(),
            declaration.location.line,
            declaration.location.column
        )),
    ];
    if let Some(module_id) = &declaration.module {
        if let Some(module) = codebase.module(module_id) {
            rows.push(DetailRow::plain(format!(
                "Module: {}",
                module.qualified_name
            )));
        }
    }
    if let Some(alias) = &declaration.alias_of {
        rows.push(DetailRow::plain(format!("Alias of: {alias}")));
    }
    rows
}

fn append_generics(rows: &mut Vec<DetailRow>, declaration: &TypeDecl) {
    if declaration.generics.is_empty() {
        return;
    }
    rows.push(DetailRow::plain(String::new()));
    rows.push(DetailRow::plain(format!(
        "Generics ({})",
        declaration.generics.len()
    )));
    for generic in &declaration.generics {
        rows.push(DetailRow::plain(format!("  {}", generic_text(generic))));
    }
}

fn append_fields(rows: &mut Vec<DetailRow>, declaration: &TypeDecl) {
    if declaration.fields.is_empty() {
        return;
    }
    rows.push(DetailRow::plain(String::new()));
    rows.push(DetailRow::plain(format!(
        "Fields ({})",
        declaration.fields.len()
    )));
    for field in &declaration.fields {
        let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
        rows.push(DetailRow::plain(format!(
            "  {} {}: {}  (line {})",
            field.visibility, field.name, type_name, field.location.line
        )));
    }
}

fn append_variants(rows: &mut Vec<DetailRow>, declaration: &TypeDecl) {
    if declaration.variants.is_empty() {
        return;
    }
    rows.push(DetailRow::plain(String::new()));
    rows.push(DetailRow::plain(format!(
        "Variants ({})",
        declaration.variants.len()
    )));
    for variant in &declaration.variants {
        rows.push(DetailRow::plain(format!(
            "  {}  (line {})",
            variant.name, variant.location.line
        )));
        for field in &variant.fields {
            let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
            rows.push(DetailRow::plain(format!(
                "    {}: {}",
                field.name, type_name
            )));
        }
    }
}

fn append_methods(rows: &mut Vec<DetailRow>, declaration: &TypeDecl, metrics: Option<&Metrics>) {
    if declaration.methods.is_empty() {
        return;
    }
    rows.push(DetailRow::plain(String::new()));
    rows.push(DetailRow::plain(format!(
        "Methods ({})",
        declaration.methods.len()
    )));
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
        let text = format!(
            "  {} {}{}{}({}){}  (line {})",
            method.visibility,
            if method.is_static { "static " } else { "" },
            method.name,
            generic_text,
            parameters,
            return_text,
            method.location.line
        );
        let value = metrics
            .and_then(|metrics| metrics.method_score(&declaration.qualified_name, &method.name));
        rows.push(DetailRow::scored(text, score_of(metrics, value)));
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

    fn detail_lines(codebase: &Codebase, declaration: &TypeDecl) -> Vec<String> {
        detail_rows(codebase, declaration, None)
            .into_iter()
            .map(|row| row.text)
            .collect()
    }

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
