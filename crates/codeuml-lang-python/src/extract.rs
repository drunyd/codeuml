//! Parsing one Python file into [`FileFacts`].

use std::path::Path;

use codeuml_core::SourceLocation;
use tree_sitter::{Node, Parser, Tree};

use crate::annotation::{dotted_path, node_location, node_text, parse_annotation, Ctx};
use crate::facts::{
    visibility_of, Annotation, ClassFact, ClassKind, FieldFact, FileFacts, Import, ImportedName,
    MethodFact, NameRef, ParamFact,
};

/// The file could not be parsed as Python.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseError {
    pub message: String,
    pub location: SourceLocation,
}

const TYPE_VAR_FACTORIES: &[&str] = &["TypeVar", "ParamSpec", "TypeVarTuple", "NewType"];
const INTERFACE_BASES: &[&str] = &["Protocol", "ABC"];
const ENUM_BASES: &[&str] = &["Enum", "IntEnum", "StrEnum", "Flag", "IntFlag"];

pub(crate) fn extract(source: &str, file: &Path) -> Result<FileFacts, ParseError> {
    let tree = parse_tree(source, file)?;
    let ctx = Ctx {
        source: source.as_bytes(),
        file,
    };
    let mut walker = Walker {
        ctx: &ctx,
        outer: Vec::new(),
        facts: FileFacts::default(),
    };
    walker.visit(tree.root_node(), false);
    Ok(walker.facts)
}

fn parse_tree(source: &str, file: &Path) -> Result<Tree, ParseError> {
    let whole_file = SourceLocation::new(file, 1, 1, 1, 1);
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|error| ParseError {
            message: format!("Python grammar is unavailable: {error}"),
            location: whole_file.clone(),
        })?;
    let tree = parser.parse(source, None).ok_or_else(|| ParseError {
        message: "parsing did not finish".to_owned(),
        location: whole_file,
    })?;
    match first_error(tree.root_node()) {
        None => Ok(tree),
        Some(node) => Err(ParseError {
            message: "invalid Python syntax".to_owned(),
            location: node_location(
                &Ctx {
                    source: source.as_bytes(),
                    file,
                },
                node,
            ),
        }),
    }
}

fn first_identifier(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = first_identifier(child) {
            return Some(found);
        }
    }
    None
}

fn first_error(node: Node<'_>) -> Option<Node<'_>> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    if !node.has_error() {
        return None;
    }
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find_map(first_error);
    found.or(Some(node))
}

struct Walker<'a> {
    ctx: &'a Ctx<'a>,
    outer: Vec<String>,
    facts: FileFacts,
}

impl Walker<'_> {
    /// Imports are collected everywhere (including inside functions and
    /// `if TYPE_CHECKING:` blocks); classes and type variables only outside
    /// function bodies.
    fn visit(&mut self, node: Node<'_>, in_function: bool) {
        match node.kind() {
            "import_statement" => self.facts.imports.extend(import_statement(node, self.ctx)),
            "import_from_statement" => self.facts.imports.extend(from_statement(node, self.ctx)),
            "function_definition" => self.visit_function(node, in_function),
            "class_definition" if !in_function => self.visit_class(node),
            "assignment" if !in_function => self.visit_assignment(node),
            "type_alias_statement" if !in_function => self.visit_type_alias(node),
            _ => self.visit_children(node, in_function),
        }
    }

    fn visit_children(&mut self, node: Node<'_>, in_function: bool) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.visit(child, in_function);
        }
    }

    /// A module-level function defines a name; its body is only searched for
    /// imports.
    fn visit_function(&mut self, node: Node<'_>, in_function: bool) {
        if !in_function && self.outer.is_empty() {
            self.define(node.child_by_field_name("name"));
        }
        self.note_type_parameters(node.child_by_field_name("type_parameters"));
        self.visit_children(node, true);
    }

    fn visit_assignment(&mut self, node: Node<'_>) {
        self.note_type_var(node);
        if self.outer.is_empty() {
            self.define(
                node.child_by_field_name("left")
                    .filter(|left| left.kind() == "identifier"),
            );
        }
    }

    /// `type Alias[T] = ...` defines `Alias` and the type parameter `T`.
    fn visit_type_alias(&mut self, node: Node<'_>) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        if self.outer.is_empty() {
            self.define(first_identifier(left));
        }
        let parameters = left
            .named_child(0)
            .filter(|generic| generic.kind() == "generic_type")
            .and_then(|generic| generic.named_child(1));
        self.note_type_parameters(parameters);
    }

    fn define(&mut self, name: Option<Node<'_>>) {
        if let Some(name) = name {
            self.facts
                .defined
                .push(node_text(self.ctx, name).to_owned());
        }
    }

    /// PEP 695 type parameters (`class A[T]`, `def f[U]`) are type variables.
    fn note_type_parameters(&mut self, parameters: Option<Node<'_>>) {
        let Some(parameters) = parameters else {
            return;
        };
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            let name = first_identifier(parameter).map(|name| node_text(self.ctx, name));
            if let Some(name) = name.filter(|name| !self.facts.type_vars.iter().any(|v| v == name))
            {
                self.facts.type_vars.push(name.to_owned());
            }
        }
    }

    fn visit_class(&mut self, node: Node<'_>) {
        let Some(class) = class_fact(node, self.ctx, &self.outer) else {
            return;
        };
        self.note_type_parameters(node.child_by_field_name("type_parameters"));
        let name = class.name.clone();
        if !self
            .facts
            .classes
            .iter()
            .any(|existing| existing.local_name() == class.local_name())
        {
            self.facts.classes.push(class);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.outer.push(name);
            self.visit_children(body, false);
            self.outer.pop();
        }
    }

    fn note_type_var(&mut self, assignment: Node<'_>) {
        let target = assignment.child_by_field_name("left");
        let factory = assignment
            .child_by_field_name("right")
            .filter(|right| right.kind() == "call")
            .and_then(|call| call.child_by_field_name("function"))
            .and_then(|function| dotted_path(function, self.ctx))
            .and_then(|path| path.last().cloned());
        let is_type_var = factory
            .as_deref()
            .is_some_and(|name| TYPE_VAR_FACTORIES.contains(&name));
        if let (true, Some(target)) = (is_type_var, target) {
            self.facts
                .type_vars
                .push(node_text(self.ctx, target).to_owned());
        }
    }
}

// ----- imports ---------------------------------------------------------------

fn segments(text: &str) -> Vec<String> {
    text.split('.')
        .map(|segment| segment.trim().to_owned())
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// `import a.b`, `import a.b as c`, `import a, b`.
fn import_statement(node: Node<'_>, ctx: &Ctx<'_>) -> Vec<Import> {
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor)
        .filter_map(|name| imported(name, ctx))
        .map(|(path, alias)| Import::Module {
            path: segments(&path),
            alias,
        })
        .collect()
}

/// The imported name and its alias from a `dotted_name` or `aliased_import`.
fn imported(node: Node<'_>, ctx: &Ctx<'_>) -> Option<(String, Option<String>)> {
    if node.kind() == "aliased_import" {
        let name = node_text(ctx, node.child_by_field_name("name")?).to_owned();
        let alias = node
            .child_by_field_name("alias")
            .map(|a| node_text(ctx, a).to_owned());
        return Some((name, alias));
    }
    Some((node_text(ctx, node).to_owned(), None))
}

/// `from m import a, b as c`, `from . import x`, `from ..m import *`.
fn from_statement(node: Node<'_>, ctx: &Ctx<'_>) -> Option<Import> {
    let (level, module) = relative_module(node.child_by_field_name("module_name")?, ctx);
    let mut cursor = node.walk();
    let names = node
        .children_by_field_name("name", &mut cursor)
        .filter_map(|name| imported(name, ctx))
        .map(|(name, alias)| ImportedName { name, alias })
        .collect();
    let mut cursor = node.walk();
    let wildcard = node
        .named_children(&mut cursor)
        .any(|child| child.kind() == "wildcard_import");
    Some(Import::From {
        level,
        module,
        names,
        wildcard,
    })
}

/// The leading-dot count and the module path of a `from` source.
fn relative_module(node: Node<'_>, ctx: &Ctx<'_>) -> (usize, Vec<String>) {
    if node.kind() != "relative_import" {
        return (0, segments(node_text(ctx, node)));
    }
    let mut cursor = node.walk();
    let mut level = 0;
    let mut module = Vec::new();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "import_prefix" => level = node_text(ctx, child).matches('.').count(),
            _ => module = segments(node_text(ctx, child)),
        }
    }
    (level, module)
}

// ----- classes ---------------------------------------------------------------

fn class_fact(node: Node<'_>, ctx: &Ctx<'_>, outer: &[String]) -> Option<ClassFact> {
    let name = node_text(ctx, node.child_by_field_name("name")?).to_owned();
    let (bases, metaclass) = node
        .child_by_field_name("superclasses")
        .map(|list| superclasses(list, ctx))
        .unwrap_or_default();
    let kind = class_kind(&bases, metaclass.as_deref());
    let members = node
        .child_by_field_name("body")
        .map(|body| class_members(body, ctx, kind))
        .unwrap_or_default();
    Some(ClassFact {
        visibility: visibility_of(&name),
        name,
        outer: outer.to_vec(),
        kind,
        bases,
        fields: members.fields,
        variants: members.variants,
        methods: members.methods,
        location: node_location(ctx, node),
    })
}

/// Base classes and the `metaclass=` keyword of a class statement.
fn superclasses(list: Node<'_>, ctx: &Ctx<'_>) -> (Vec<NameRef>, Option<String>) {
    let mut bases = Vec::new();
    let mut metaclass = None;
    let mut cursor = list.walk();
    for argument in list.named_children(&mut cursor) {
        if argument.kind() == "keyword_argument" {
            metaclass = metaclass.or_else(|| metaclass_name(argument, ctx));
        } else if let Some(base) = base_ref(argument, ctx) {
            bases.push(base);
        }
    }
    (bases, metaclass)
}

fn metaclass_name(argument: Node<'_>, ctx: &Ctx<'_>) -> Option<String> {
    let name = argument.child_by_field_name("name")?;
    let value = argument.child_by_field_name("value")?;
    (node_text(ctx, name) == "metaclass")
        .then(|| dotted_path(value, ctx))
        .flatten()
        .and_then(|path| path.last().cloned())
}

/// A base class reference: a name, a dotted name, or the head of `Base[T]`.
fn base_ref(node: Node<'_>, ctx: &Ctx<'_>) -> Option<NameRef> {
    let head = if node.kind() == "subscript" {
        node.child_by_field_name("value")?
    } else {
        node
    };
    Some(NameRef {
        path: dotted_path(head, ctx)?,
        location: node_location(ctx, node),
    })
}

fn class_kind(bases: &[NameRef], metaclass: Option<&str>) -> ClassKind {
    let has_base = |names: &[&str]| {
        bases.iter().any(|base| {
            base.path
                .last()
                .is_some_and(|last| names.contains(&last.as_str()))
        })
    };
    if has_base(INTERFACE_BASES) || metaclass == Some("ABCMeta") {
        ClassKind::Interface
    } else if has_base(ENUM_BASES) {
        ClassKind::Enum
    } else {
        ClassKind::Class
    }
}

#[derive(Default)]
struct Members {
    fields: Vec<FieldFact>,
    variants: Vec<(String, SourceLocation)>,
    methods: Vec<MethodFact>,
}

fn class_members(body: Node<'_>, ctx: &Ctx<'_>, kind: ClassKind) -> Members {
    let mut members = Members::default();
    let mut init_fields = Vec::new();
    let mut cursor = body.walk();
    for statement in body.named_children(&mut cursor) {
        match statement.kind() {
            "expression_statement" => class_attribute(statement, ctx, kind, &mut members),
            "function_definition" => {
                add_method(statement, &[], ctx, &mut members, &mut init_fields)
            }
            "decorated_definition" => {
                decorated_method(statement, ctx, &mut members, &mut init_fields)
            }
            _ => {}
        }
    }
    for field in init_fields {
        push_field(&mut members.fields, field);
    }
    members
}

fn decorated_method(
    node: Node<'_>,
    ctx: &Ctx<'_>,
    members: &mut Members,
    init_fields: &mut Vec<FieldFact>,
) {
    let Some(definition) = node
        .child_by_field_name("definition")
        .filter(|definition| definition.kind() == "function_definition")
    else {
        return;
    };
    add_method(
        definition,
        &decorator_names(node, ctx),
        ctx,
        members,
        init_fields,
    );
}

/// Decorator names without `@` or call arguments: `@a.b(1)` is `a.b`.
fn decorator_names(decorated: Node<'_>, ctx: &Ctx<'_>) -> Vec<String> {
    let mut cursor = decorated.walk();
    decorated
        .children(&mut cursor)
        .filter(|child| child.kind() == "decorator")
        .map(|decorator| {
            let text = node_text(ctx, decorator).trim_start_matches('@');
            text.split('(').next().unwrap_or("").trim().to_owned()
        })
        .collect()
}

fn add_method(
    node: Node<'_>,
    decorators: &[String],
    ctx: &Ctx<'_>,
    members: &mut Members,
    init_fields: &mut Vec<FieldFact>,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = node_text(ctx, name_node).to_owned();
    let has_decorator = |wanted: &str| decorators.iter().any(|decorator| decorator == wanted);
    let is_static = has_decorator("staticmethod") || has_decorator("classmethod");
    let all_params = node
        .child_by_field_name("parameters")
        .map(|parameters| param_facts(parameters, ctx))
        .unwrap_or_default();
    if name == "__init__" {
        init_fields.extend(init_attributes(node, &all_params, ctx));
    }
    if members.methods.iter().any(|method| method.name == name) {
        return;
    }
    // Only a static method has no receiver (`self`/`cls`) to drop.
    let skip_receiver = usize::from(!has_decorator("staticmethod"));
    members.methods.push(MethodFact {
        visibility: visibility_of(&name),
        params: all_params.into_iter().skip(skip_receiver).collect(),
        returns: node
            .child_by_field_name("return_type")
            .map(|returns| parse_annotation(returns, ctx)),
        is_static,
        location: node_location(ctx, node),
        name,
    });
}

// ----- class attributes ------------------------------------------------------

fn class_attribute(statement: Node<'_>, ctx: &Ctx<'_>, kind: ClassKind, members: &mut Members) {
    let Some(assignment) = statement
        .named_child(0)
        .filter(|child| child.kind() == "assignment")
    else {
        return;
    };
    let Some(target) = assignment
        .child_by_field_name("left")
        .filter(|left| left.kind() == "identifier")
    else {
        return;
    };
    let name = node_text(ctx, target).to_owned();
    if is_dunder(&name) {
        return;
    }
    let location = node_location(ctx, target);
    if kind == ClassKind::Enum {
        members.variants.push((name, location));
        return;
    }
    let annotation = assignment
        .child_by_field_name("type")
        .map(|annotation| parse_annotation(annotation, ctx));
    push_field(
        &mut members.fields,
        FieldFact {
            visibility: visibility_of(&name),
            name,
            annotation,
            location,
        },
    );
}

fn is_dunder(name: &str) -> bool {
    name.len() > 4 && name.starts_with("__") && name.ends_with("__")
}

fn push_field(fields: &mut Vec<FieldFact>, field: FieldFact) {
    if !fields.iter().any(|existing| existing.name == field.name) {
        fields.push(field);
    }
}

/// `self.x = ...` and `self.x: T = ...` inside `__init__`. A bare `self.x = x`
/// takes the annotation of the parameter `x` when it has one.
fn init_attributes(function: Node<'_>, params: &[ParamFact], ctx: &Ctx<'_>) -> Vec<FieldFact> {
    let Some(receiver) = params.first().map(|param| param.name.as_str()) else {
        return Vec::new();
    };
    let Some(body) = function.child_by_field_name("body") else {
        return Vec::new();
    };
    let mut assignments = Vec::new();
    collect_assignments(body, &mut assignments);
    assignments
        .into_iter()
        .filter_map(|assignment| self_attribute(assignment, receiver, params, ctx))
        .collect()
}

fn collect_assignments<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    if matches!(node.kind(), "function_definition" | "class_definition") {
        return;
    }
    if node.kind() == "assignment" {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_assignments(child, out);
    }
}

fn self_attribute(
    assignment: Node<'_>,
    receiver: &str,
    params: &[ParamFact],
    ctx: &Ctx<'_>,
) -> Option<FieldFact> {
    let left = assignment.child_by_field_name("left")?;
    left.child_by_field_name("object")
        .filter(|object| object.kind() == "identifier" && node_text(ctx, *object) == receiver)?;
    let name_node = left.child_by_field_name("attribute")?;
    let name = node_text(ctx, name_node).to_owned();
    let annotation = assignment
        .child_by_field_name("type")
        .map(|annotation| parse_annotation(annotation, ctx))
        .or_else(|| annotation_from_parameter(assignment, params, ctx));
    Some(FieldFact {
        visibility: visibility_of(&name),
        name,
        annotation,
        location: node_location(ctx, left),
    })
}

fn annotation_from_parameter(
    assignment: Node<'_>,
    params: &[ParamFact],
    ctx: &Ctx<'_>,
) -> Option<Annotation> {
    let right = assignment
        .child_by_field_name("right")
        .filter(|right| right.kind() == "identifier")?;
    let name = node_text(ctx, right);
    params
        .iter()
        .find(|param| param.name == name)
        .and_then(|param| param.annotation.clone())
}

// ----- parameters ------------------------------------------------------------

fn param_facts(parameters: Node<'_>, ctx: &Ctx<'_>) -> Vec<ParamFact> {
    let mut cursor = parameters.walk();
    parameters
        .named_children(&mut cursor)
        .filter_map(|parameter| param_fact(parameter, ctx))
        .collect()
}

fn param_fact(parameter: Node<'_>, ctx: &Ctx<'_>) -> Option<ParamFact> {
    let name_node = match parameter.kind() {
        "identifier" | "list_splat_pattern" | "dictionary_splat_pattern" => parameter,
        "typed_parameter" => parameter.named_child(0)?,
        "default_parameter" | "typed_default_parameter" => parameter.child_by_field_name("name")?,
        _ => return None,
    };
    Some(ParamFact {
        name: parameter_name(name_node, ctx)?,
        annotation: parameter
            .child_by_field_name("type")
            .map(|annotation| parse_annotation(annotation, ctx)),
    })
}

fn parameter_name(node: Node<'_>, ctx: &Ctx<'_>) -> Option<String> {
    let inner = || node.named_child(0).map(|inner| node_text(ctx, inner));
    match node.kind() {
        "identifier" => Some(node_text(ctx, node).to_owned()),
        "list_splat_pattern" => Some(format!("*{}", inner()?)),
        "dictionary_splat_pattern" => Some(format!("**{}", inner()?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use codeuml_core::Visibility;

    use super::*;

    fn facts(source: &str) -> FileFacts {
        extract(source, Path::new("m.py")).expect("valid python")
    }

    fn class<'a>(facts: &'a FileFacts, local_name: &str) -> &'a ClassFact {
        facts
            .classes
            .iter()
            .find(|class| class.local_name() == local_name)
            .unwrap_or_else(|| panic!("missing class {local_name}"))
    }

    fn names<T>(items: &[T], name: impl Fn(&T) -> &str) -> Vec<String> {
        items.iter().map(|item| name(item).to_owned()).collect()
    }

    fn base_names(class: &ClassFact) -> Vec<String> {
        class.bases.iter().map(NameRef::display).collect()
    }

    fn type_text(annotation: &Option<Annotation>) -> Option<&str> {
        annotation
            .as_ref()
            .map(|annotation| annotation.text.as_str())
    }

    #[test]
    fn every_import_form_is_collected_wherever_it_appears() {
        let found = facts(
            "from __future__ import annotations\nimport os, a.b as c\nfrom . import sibling\nfrom ..pkg.mod import Thing as Alias, Other\nfrom m import *\nif TYPE_CHECKING:\n    from x import Y\ndef f():\n    import late\n",
        );
        let imported = |name: &str, alias: Option<&str>| ImportedName {
            name: name.to_owned(),
            alias: alias.map(str::to_owned),
        };
        let path = |text: &str| text.split('.').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            found.imports,
            vec![
                Import::Module {
                    path: path("os"),
                    alias: None
                },
                Import::Module {
                    path: path("a.b"),
                    alias: Some("c".to_owned())
                },
                Import::From {
                    level: 1,
                    module: Vec::new(),
                    names: vec![imported("sibling", None)],
                    wildcard: false
                },
                Import::From {
                    level: 2,
                    module: path("pkg.mod"),
                    names: vec![imported("Thing", Some("Alias")), imported("Other", None)],
                    wildcard: false
                },
                Import::From {
                    level: 0,
                    module: path("m"),
                    names: Vec::new(),
                    wildcard: true
                },
                Import::From {
                    level: 0,
                    module: path("x"),
                    names: vec![imported("Y", None)],
                    wildcard: false
                },
                Import::Module {
                    path: path("late"),
                    alias: None
                },
            ]
        );
    }

    #[test]
    fn module_level_functions_and_assignments_are_recorded_as_defined_names() {
        let found = facts(
            "Alias = dict[str, int]\nT = TypeVar('T')\ndef func():\n    local = 1\n    def nested(): ...\nclass C:\n    attr = 1\n    def method(self): ...\nif FLAG:\n    def conditional(): ...\na, b = 1, 2\nobj.attr = 3\n",
        );
        assert_eq!(found.defined, ["Alias", "T", "func", "conditional"]);
    }

    #[test]
    fn pep_695_aliases_and_type_parameters_are_names_not_classes() {
        let found = facts(
            "type KeySpec = str\ntype Pair[T] = tuple[T, T]\nclass A[K, *Ts](Base[K]):\n    def f[U: int](self, x: U) -> K: ...\ndef g[**P](): ...\n",
        );
        assert_eq!(found.defined, ["KeySpec", "Pair", "g"]);
        assert_eq!(found.type_vars, ["T", "K", "Ts", "U", "P"]);
        assert_eq!(class(&found, "A").bases[0].path, ["Base"]);
    }

    #[test]
    fn type_variables_are_noted_only_at_module_level() {
        let found = facts(
            "T = TypeVar('T')\nU = typing.NewType('U', int)\nP = ParamSpec('P')\nV = 3\nW = make()\nX = f()()\ndef g():\n    Z = TypeVar('Z')\n",
        );
        assert_eq!(found.type_vars, ["T", "U", "P"]);
    }

    #[test]
    fn interfaces_and_enums_are_recognised_from_bases_and_metaclasses() {
        let found = facts(
            "class P(Protocol): ...\nclass W(abc.ABC): ...\nclass M(metaclass=ABCMeta): ...\nclass N(metaclass=abc.ABCMeta): ...\nclass E(Enum): ...\nclass F(enum.IntFlag): ...\nclass O(metaclass=Other): ...\nclass D(Base, **opts, total=False): ...\nclass S(Base, pkg.Mixin, Foo[T]): ...\nclass Plain: ...\n",
        );
        let kind = |name: &str| class(&found, name).kind;
        assert_eq!(kind("P"), ClassKind::Interface);
        assert_eq!(kind("W"), ClassKind::Interface);
        assert_eq!(kind("M"), ClassKind::Interface);
        assert_eq!(kind("N"), ClassKind::Interface);
        assert_eq!(kind("E"), ClassKind::Enum);
        assert_eq!(kind("F"), ClassKind::Enum);
        assert_eq!(kind("O"), ClassKind::Class);
        assert_eq!(kind("Plain"), ClassKind::Class);
        assert_eq!(base_names(class(&found, "D")), ["Base"]);
        assert_eq!(base_names(class(&found, "S")), ["Base", "pkg.Mixin", "Foo"]);
        assert!(base_names(class(&found, "Plain")).is_empty());
        assert_eq!(class(&found, "S").bases[2].location.line, 9);
    }

    const SHAPE: &str = "class Shape:\n    sides: int = 0\n    name = 'x'\n    _private = 1\n    __slots__ = ()\n    items: List['Item']\n    a, b = 1, 2\n    obj.attr = 3\n\n    def __init__(self, label: str, count, *args: int, flag: bool = False, **kw):\n        self.label = label\n        self.count = count\n        self._cache: Dict[str, Item] = {}\n        self.other.attr = 1\n        helper.value = 2\n        self.sides = 4\n        def inner():\n            self.skipped = 1\n        if flag:\n            self.flagged = flag\n";

    #[test]
    fn class_and_instance_attributes_become_fields_in_order() {
        let found = facts(SHAPE);
        let shape = class(&found, "Shape");
        assert_eq!(
            names(&shape.fields, |field| &field.name),
            ["sides", "name", "_private", "items", "label", "count", "_cache", "flagged"]
        );
        let field = |name: &str| {
            shape
                .fields
                .iter()
                .find(|field| field.name == name)
                .expect("field")
        };
        assert_eq!(type_text(&field("sides").annotation), Some("int"));
        assert_eq!(type_text(&field("name").annotation), None);
        assert_eq!(field("_private").visibility, Visibility::Private);
        assert_eq!(field("name").visibility, Visibility::Public);
        let items = field("items").annotation.as_ref().expect("annotation");
        assert_eq!(items.text, "List['Item']");
        assert_eq!(items.refs.len(), 2);
        // `self.x = param` borrows the parameter's annotation; otherwise none.
        assert_eq!(type_text(&field("label").annotation), Some("str"));
        assert_eq!(type_text(&field("count").annotation), None);
        assert_eq!(type_text(&field("flagged").annotation), Some("bool"));
        assert_eq!(
            type_text(&field("_cache").annotation),
            Some("Dict[str, Item]")
        );
        assert_eq!(field("label").location.line, 11);
    }

    #[test]
    fn a_class_level_attribute_wins_over_an_instance_attribute_of_the_same_name() {
        let found = facts(
            "class A:\n    x: int = 0\n    def __init__(self, x: str):\n        self.x = x\n",
        );
        let fields = &class(&found, "A").fields;
        assert_eq!(fields.len(), 1);
        assert_eq!(type_text(&fields[0].annotation), Some("int"));
    }

    #[test]
    fn init_attributes_need_a_receiver_and_a_body() {
        let found = facts("class A:\n    def __init__(): ...\n    def __init__2(self): ...\n");
        assert!(class(&found, "A").fields.is_empty());
    }

    const METHODS: &str = "class Service:\n    def area(self) -> float: ...\n    @property\n    def perimeter(self) -> float: ...\n    @staticmethod\n    def make(size: int, label) -> 'Service': ...\n    @classmethod\n    def build(cls, spec: Spec) -> Service: ...\n    @functools.lru_cache(maxsize=1)\n    def cached(self): ...\n    def area(self) -> int: ...\n    def _helper(self): ...\n    async def run(self, /, a, *, b, c=1, d: int = 2): ...\n    def __repr__(self): ...\n    def sigs(self, (x, y)): ...\n";

    #[test]
    fn methods_keep_their_order_visibility_and_decorator_semantics() {
        let found = facts(METHODS);
        let service = class(&found, "Service");
        assert_eq!(
            names(&service.methods, |method| &method.name),
            [
                "area",
                "perimeter",
                "make",
                "build",
                "cached",
                "_helper",
                "run",
                "__repr__",
                "sigs"
            ]
        );
        let method = |name: &str| {
            service
                .methods
                .iter()
                .find(|method| method.name == name)
                .expect("method")
        };
        // The duplicate `area` is ignored: the first definition wins.
        assert_eq!(type_text(&method("area").returns), Some("float"));
        assert!(!method("area").is_static && !method("cached").is_static);
        assert!(method("make").is_static && method("build").is_static);
        // A static method keeps every parameter; others drop the receiver.
        assert_eq!(
            names(&method("make").params, |param| &param.name),
            ["size", "label"]
        );
        assert_eq!(
            names(&method("build").params, |param| &param.name),
            ["spec"]
        );
        assert_eq!(
            names(&method("run").params, |param| &param.name),
            ["a", "b", "c", "d"]
        );
        assert_eq!(type_text(&method("run").params[3].annotation), Some("int"));
        assert!(method("area").params.is_empty());
        // A tuple-pattern parameter (Python 2) is not a named parameter.
        assert!(method("sigs").params.is_empty());
        assert_eq!(method("_helper").visibility, Visibility::Private);
        assert_eq!(method("__repr__").visibility, Visibility::Public);
        assert_eq!(method("area").location.line, 2);
    }

    #[test]
    fn variadic_parameters_keep_their_stars_and_annotations() {
        let found = facts("class A:\n    def f(self, *args: int, **kw: str): ...\n    def g(self, *rest, **opts): ...\n");
        let a = class(&found, "A");
        assert_eq!(
            names(&a.methods[0].params, |param| &param.name),
            ["*args", "**kw"]
        );
        assert_eq!(type_text(&a.methods[0].params[0].annotation), Some("int"));
        assert_eq!(type_text(&a.methods[0].params[1].annotation), Some("str"));
        assert_eq!(
            names(&a.methods[1].params, |param| &param.name),
            ["*rest", "**opts"]
        );
    }

    #[test]
    fn nested_and_conditional_classes_are_found_but_function_local_ones_are_not() {
        let found = facts(
            "class Outer:\n    class Inner:\n        def m(self): ...\n    def method(self): ...\n\ntry:\n    class Fast: ...\nexcept ImportError:\n    class Fast: ...\n\nif FLAG:\n    class Flagged: ...\n\n@decorator\nclass Decorated: ...\n\ndef func():\n    class Hidden: ...\n\nclass Dup: ...\nclass Dup:\n    x = 1\n",
        );
        assert_eq!(
            names(&found.classes, |class| &class.name),
            ["Outer", "Inner", "Fast", "Flagged", "Decorated", "Dup"]
        );
        let inner = class(&found, "Outer.Inner");
        assert_eq!(inner.outer, ["Outer"]);
        assert_eq!(inner.methods.len(), 1);
        assert_eq!(class(&found, "Outer").methods[0].name, "method");
        assert!(
            class(&found, "Dup").fields.is_empty(),
            "first definition wins"
        );
    }

    #[test]
    fn enum_members_are_variants_and_private_or_dunder_names_are_handled() {
        let found = facts(
            "class Color(Enum):\n    RED = 1\n    GREEN: int = 2\n    __x__ = 0\n    def label(self): ...\n",
        );
        let color = class(&found, "Color");
        assert_eq!(
            names(&color.variants, |variant| &variant.0),
            ["RED", "GREEN"]
        );
        assert!(color.fields.is_empty());
        assert_eq!(color.methods.len(), 1);
        assert_eq!(color.variants[0].1.line, 2);
        assert_eq!(color.visibility, Visibility::Public);
        assert_eq!(
            class(&facts("class _Hidden: ..."), "_Hidden").visibility,
            Visibility::Private
        );
    }

    #[test]
    fn empty_files_and_odd_class_shapes_are_fine() {
        assert_eq!(facts(""), FileFacts::default());
        let found = facts("class Bare: pass\nclass Tuple:\n    a, b = 1, 2\nx: int = 3\n");
        assert_eq!(found.classes.len(), 2);
        assert!(class(&found, "Tuple").fields.is_empty());
    }

    #[test]
    fn syntax_errors_are_reported_at_the_first_bad_spot() {
        let error =
            extract("class A:\n    pass\n\ndef (:\n", Path::new("bad.py")).expect_err("syntax");
        assert_eq!(error.message, "invalid Python syntax");
        assert_eq!(error.location.file, Path::new("bad.py"));
        assert_eq!(error.location.line, 4);
        let missing = extract("class A(\n", Path::new("bad.py")).expect_err("unterminated");
        assert_eq!(missing.location.line, 1);
        assert!(extract("x = = 1\n", Path::new("bad.py")).is_err());
    }
}
