//! Reading type annotations: their display text and the names they mention.

use std::path::Path;

use codeuml_core::SourceLocation;
use tree_sitter::{Node, Parser};

use crate::facts::{location, Annotation, NameRef};

/// A quoted annotation (`"Foo | None"`) is read once; quotes inside it are not.
const MAX_FORWARD_DEPTH: usize = 1;

/// The source being read and the file it belongs to.
pub(crate) struct Ctx<'a> {
    pub source: &'a [u8],
    pub file: &'a Path,
}

pub(crate) fn node_text<'a>(ctx: &Ctx<'a>, node: Node<'_>) -> &'a str {
    node.utf8_text(ctx.source).unwrap_or("")
}

pub(crate) fn node_location(ctx: &Ctx<'_>, node: Node<'_>) -> SourceLocation {
    let (start, end) = (node.start_position(), node.end_position());
    location(ctx.file, (start.row, start.column), (end.row, end.column))
}

/// Read an annotation node (a `type` node or any expression).
pub(crate) fn parse_annotation(node: Node<'_>, ctx: &Ctx<'_>) -> Annotation {
    let mut refs = Vec::new();
    collect(node, ctx, 0, &mut refs);
    Annotation {
        text: strip_quotes(node_text(ctx, node)),
        refs,
    }
}

/// The dotted name of an identifier or attribute chain, `a.b.C`.
pub(crate) fn dotted_path(node: Node<'_>, ctx: &Ctx<'_>) -> Option<Vec<String>> {
    match node.kind() {
        "identifier" => Some(vec![node_text(ctx, node).to_owned()]),
        "attribute" => {
            let mut path = dotted_path(node.child_by_field_name("object")?, ctx)?;
            path.push(node_text(ctx, node.child_by_field_name("attribute")?).to_owned());
            Some(path)
        }
        _ => None,
    }
}

fn strip_quotes(text: &str) -> String {
    let trimmed = text.trim();
    let quoted = ['"', '\''].iter().any(|quote| {
        trimmed.len() >= 2 && trimmed.starts_with(*quote) && trimmed.ends_with(*quote)
    });
    if quoted {
        trimmed[1..trimmed.len() - 1].trim().to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn collect(node: Node<'_>, ctx: &Ctx<'_>, depth: usize, out: &mut Vec<NameRef>) {
    match node.kind() {
        "identifier" | "attribute" => push_dotted(node, ctx, out),
        "subscript" | "generic_type" => collect_generic(node, ctx, depth, out),
        "string" => collect_forward(node, ctx, depth, out),
        "none" | "integer" | "float" | "true" | "false" | "ellipsis" | "comment" => {}
        _ => collect_children(node, ctx, depth, out),
    }
}

fn push_dotted(node: Node<'_>, ctx: &Ctx<'_>, out: &mut Vec<NameRef>) {
    if let Some(path) = dotted_path(node, ctx) {
        out.push(NameRef {
            path,
            location: node_location(ctx, node),
        });
    }
}

fn collect_children(node: Node<'_>, ctx: &Ctx<'_>, depth: usize, out: &mut Vec<NameRef>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect(child, ctx, depth, out);
    }
}

/// `Head[args]`: the head, then the arguments, except that `Literal` arguments
/// are values and `Annotated` carries a type only in its first argument.
fn collect_generic(node: Node<'_>, ctx: &Ctx<'_>, depth: usize, out: &mut Vec<NameRef>) {
    let mut cursor = node.walk();
    let mut children = node.named_children(&mut cursor);
    let Some(head) = children.next() else {
        return;
    };
    let head_name = dotted_path(head, ctx).and_then(|path| path.last().cloned());
    if head_name.as_deref() == Some("Literal") {
        return;
    }
    collect(head, ctx, depth, out);
    let limit = if head_name.as_deref() == Some("Annotated") {
        1
    } else {
        usize::MAX
    };
    for argument in generic_arguments(children).into_iter().take(limit) {
        collect(argument, ctx, depth, out);
    }
}

/// Arguments of a generic: `type_parameter` wraps them in annotation position.
fn generic_arguments<'t>(rest: impl Iterator<Item = Node<'t>>) -> Vec<Node<'t>> {
    let mut arguments = Vec::new();
    for child in rest {
        if child.kind() == "type_parameter" {
            let mut cursor = child.walk();
            arguments.extend(child.named_children(&mut cursor));
        } else {
            arguments.push(child);
        }
    }
    arguments
}

/// A quoted annotation is parsed as an expression of its own; every name found
/// is located at the string.
fn collect_forward(node: Node<'_>, ctx: &Ctx<'_>, depth: usize, out: &mut Vec<NameRef>) {
    if depth >= MAX_FORWARD_DEPTH {
        return;
    }
    let text = strip_quotes(node_text(ctx, node));
    let Some(tree) = parse_expression(&text) else {
        return;
    };
    let inner = Ctx {
        source: text.as_bytes(),
        file: ctx.file,
    };
    let mut found = Vec::new();
    collect(tree.root_node(), &inner, depth + 1, &mut found);
    let place = node_location(ctx, node);
    out.extend(found.into_iter().map(|reference| NameRef {
        path: reference.path,
        location: place.clone(),
    }));
}

fn parse_expression(text: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(text, None)?;
    (!tree.root_node().has_error()).then_some(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Annotation of `x: <annotation>` parsed from a one-line source.
    fn annotation_of(annotation: &str) -> Annotation {
        let source = format!("x: {annotation}\n");
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("language");
        let tree = parser.parse(&source, None).expect("tree");
        let statement = tree.root_node().named_child(0).expect("statement");
        let assignment = statement.named_child(0).expect("assignment");
        let node = assignment.child_by_field_name("type").expect("type");
        let ctx = Ctx {
            source: source.as_bytes(),
            file: Path::new("a.py"),
        };
        parse_annotation(node, &ctx)
    }

    fn names(annotation: &str) -> Vec<String> {
        annotation_of(annotation)
            .refs
            .iter()
            .map(NameRef::display)
            .collect()
    }

    #[test]
    fn plain_dotted_and_generic_names_are_collected_in_order() {
        assert_eq!(names("int"), ["int"]);
        assert_eq!(names("Optional[Foo]"), ["Optional", "Foo"]);
        assert_eq!(
            names("List[Dict[str, Bar]]"),
            ["List", "Dict", "str", "Bar"]
        );
        assert_eq!(names("typing.List[a.b.C]"), ["typing.List", "a.b.C"]);
        assert_eq!(names("Foo | None"), ["Foo"]);
        assert_eq!(names("Callable[[A, B], C]"), ["Callable", "A", "B", "C"]);
        assert!(names("None").is_empty());
    }

    #[test]
    fn literal_values_and_annotated_metadata_are_not_types() {
        assert!(names("Literal['x', 'y']").is_empty());
        assert!(names("typing.Literal['x']").is_empty());
        assert_eq!(names("Annotated[Foo, 'meta', Other]"), ["Annotated", "Foo"]);
    }

    #[test]
    fn quoted_annotations_are_read_once_and_located_at_the_string() {
        let quoted = annotation_of("'Foo | None'");
        assert_eq!(quoted.text, "Foo | None");
        assert_eq!(quoted.refs.len(), 1);
        assert_eq!(quoted.refs[0].path, ["Foo"]);
        assert_eq!(quoted.refs[0].location.column, 4);

        assert_eq!(names("List['Foo']"), ["List", "Foo"]);
        assert_eq!(names("\"Dict[str, 'X']\""), ["Dict", "str"]);
        assert!(names("'not a type at all'").is_empty());
    }

    #[test]
    fn display_text_is_the_source_with_one_layer_of_quotes_removed() {
        assert_eq!(annotation_of("Optional[Foo]").text, "Optional[Foo]");
        assert_eq!(annotation_of("\"Foo\"").text, "Foo");
        assert_eq!(strip_quotes("'"), "'");
        assert_eq!(strip_quotes("  plain  "), "plain");
    }

    #[test]
    fn dotted_paths_require_a_plain_chain() {
        let source = "a.b.c\nf().x\n";
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("language");
        let tree = parser.parse(source, None).expect("tree");
        let ctx = Ctx {
            source: source.as_bytes(),
            file: Path::new("a.py"),
        };
        let root = tree.root_node();
        let chain = root
            .named_child(0)
            .and_then(|n| n.named_child(0))
            .expect("chain");
        assert_eq!(
            dotted_path(chain, &ctx),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
        let call = root
            .named_child(1)
            .and_then(|n| n.named_child(0))
            .expect("call");
        assert_eq!(dotted_path(call, &ctx), None);
        assert!(dotted_path(root, &ctx).is_none());
    }
}
