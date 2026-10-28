//! Plain facts extracted from one Python file, independent of the parser and
//! of the shared model. Extraction produces them, model building and
//! resolution consume them.

use std::path::Path;

use codeuml_core::{SourceLocation, Visibility};

/// A dotted name as written in source: `pkg.mod.Class` is `["pkg", "mod", "Class"]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NameRef {
    pub path: Vec<String>,
    pub location: SourceLocation,
}

impl NameRef {
    pub(crate) fn display(&self) -> String {
        self.path.join(".")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportedName {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Import {
    /// `import a.b` or `import a.b as c`.
    Module {
        path: Vec<String>,
        alias: Option<String>,
    },
    /// `from <level dots><module> import names`; `level` counts the leading dots.
    From {
        level: usize,
        module: Vec<String>,
        names: Vec<ImportedName>,
        wildcard: bool,
    },
}

/// A type annotation: its source text and the names it refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Annotation {
    pub text: String,
    pub refs: Vec<NameRef>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldFact {
    pub name: String,
    pub annotation: Option<Annotation>,
    pub visibility: Visibility,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParamFact {
    pub name: String,
    pub annotation: Option<Annotation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MethodFact {
    pub name: String,
    pub params: Vec<ParamFact>,
    pub returns: Option<Annotation>,
    /// `@staticmethod` or `@classmethod`: no instance receiver.
    pub is_static: bool,
    pub visibility: Visibility,
    pub location: SourceLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClassKind {
    Class,
    /// Derives from `Protocol` or `ABC` (or uses `ABCMeta`).
    Interface,
    /// Derives from `Enum`, `IntEnum`, `Flag`, and friends.
    Enum,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClassFact {
    pub name: String,
    /// Names of enclosing classes, outermost first.
    pub outer: Vec<String>,
    pub kind: ClassKind,
    pub visibility: Visibility,
    pub bases: Vec<NameRef>,
    pub fields: Vec<FieldFact>,
    pub variants: Vec<(String, SourceLocation)>,
    pub methods: Vec<MethodFact>,
    pub location: SourceLocation,
}

impl ClassFact {
    /// The class name qualified by its enclosing classes, `Outer.Inner`.
    pub(crate) fn local_name(&self) -> String {
        self.outer
            .iter()
            .chain(std::iter::once(&self.name))
            .cloned()
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileFacts {
    pub imports: Vec<Import>,
    pub classes: Vec<ClassFact>,
    /// Module-level names that are type parameters (`T = TypeVar("T")`) and
    /// must not be reported as unresolved references.
    pub type_vars: Vec<String>,
    /// Module-level names bound by functions and assignments: aliases,
    /// constants, special forms. They exist but are not classes, which is
    /// different from not existing at all.
    pub defined: Vec<String>,
}

/// Python names are public unless they start with a single underscore;
/// `__dunder__` names are public.
pub(crate) fn visibility_of(name: &str) -> Visibility {
    let private = name.starts_with('_') && !(name.starts_with("__") && name.ends_with("__"));
    if private {
        Visibility::Private
    } else {
        Visibility::Public
    }
}

pub(crate) fn location(file: &Path, start: (usize, usize), end: (usize, usize)) -> SourceLocation {
    SourceLocation::new(
        file,
        line_number(start.0),
        line_number(start.1),
        line_number(end.0),
        line_number(end.1),
    )
}

fn line_number(zero_based: usize) -> u32 {
    u32::try_from(zero_based.saturating_add(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_follows_the_underscore_convention() {
        assert_eq!(visibility_of("name"), Visibility::Public);
        assert_eq!(visibility_of("_name"), Visibility::Private);
        assert_eq!(visibility_of("__mangled"), Visibility::Private);
        assert_eq!(visibility_of("__init__"), Visibility::Public);
    }

    #[test]
    fn locations_are_one_based_and_saturate() {
        let place = location(Path::new("a.py"), (0, 4), (2, 9));
        assert_eq!(
            (place.line, place.column, place.end_line, place.end_column),
            (1, 5, 3, 10)
        );
        assert_eq!(line_number(usize::MAX), u32::MAX);
    }

    #[test]
    fn names_display_dotted_and_classes_include_their_outer_classes() {
        let place = location(Path::new("a.py"), (0, 0), (0, 0));
        let reference = NameRef {
            path: vec!["pkg".to_owned(), "Base".to_owned()],
            location: place.clone(),
        };
        assert_eq!(reference.display(), "pkg.Base");

        let class = ClassFact {
            name: "Inner".to_owned(),
            outer: vec!["Outer".to_owned()],
            kind: ClassKind::Class,
            visibility: Visibility::Public,
            bases: Vec::new(),
            fields: Vec::new(),
            variants: Vec::new(),
            methods: Vec::new(),
            location: place,
        };
        assert_eq!(class.local_name(), "Outer.Inner");
    }
}
