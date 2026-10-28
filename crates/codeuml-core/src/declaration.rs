use std::fmt;
use std::path::PathBuf;

use crate::{Language, SourceLocation, SymbolId};

/// Language-neutral visibility. `Restricted` keeps the original restriction
/// text when a language exposes something more specific than crate/module
/// visibility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Crate,
    Super,
    Restricted(String),
    #[default]
    Private,
}

impl fmt::Display for Visibility {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => formatter.write_str("pub"),
            Self::Crate => formatter.write_str("pub(crate)"),
            Self::Super => formatter.write_str("pub(super)"),
            Self::Restricted(text) => write!(formatter, "pub(in {text})"),
            Self::Private => formatter.write_str("private"),
        }
    }
}

/// A source file belonging to the analyzed project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    pub path: PathBuf,
    pub language: Language,
}

impl SourceFile {
    pub fn new(path: impl Into<PathBuf>, language: Language) -> Self {
        Self {
            path: path.into(),
            language,
        }
    }
}

/// A module declaration. Module hierarchy is represented by `parent`, while
/// declarations point at their containing module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub id: SymbolId,
    pub name: String,
    pub qualified_name: String,
    pub file: PathBuf,
    pub parent: Option<SymbolId>,
    pub location: SourceLocation,
}

impl Module {
    pub fn new(
        id: SymbolId,
        name: impl Into<String>,
        qualified_name: impl Into<String>,
        file: impl Into<PathBuf>,
        location: SourceLocation,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            qualified_name: qualified_name.into(),
            file: file.into(),
            parent: None,
            location,
        }
    }

    pub fn with_parent(mut self, parent: SymbolId) -> Self {
        self.parent = Some(parent);
        self
    }
}

/// The kind of a type declaration.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Struct,
    Enum,
    Trait,
    Class,
    Interface,
    TypeAlias,
    Union,
    Other(String),
}

impl fmt::Display for TypeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Struct => formatter.write_str("struct"),
            Self::Enum => formatter.write_str("enum"),
            Self::Trait => formatter.write_str("trait"),
            Self::Class => formatter.write_str("class"),
            Self::Interface => formatter.write_str("interface"),
            Self::TypeAlias => formatter.write_str("type alias"),
            Self::Union => formatter.write_str("union"),
            Self::Other(name) => formatter.write_str(name),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<String>,
    pub default: Option<String>,
}

impl GenericParam {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            bounds: Vec::new(),
            default: None,
        }
    }

    pub fn with_bounds<I, S>(mut self, bounds: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.bounds = bounds.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }
}

/// A field on a struct, enum variant, class, or interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: SymbolId,
    pub name: String,
    pub type_name: Option<String>,
    pub visibility: Visibility,
    pub location: SourceLocation,
}

impl Field {
    pub fn new(id: SymbolId, name: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            id,
            name: name.into(),
            type_name: None,
            visibility: Visibility::Private,
            location,
        }
    }

    pub fn with_type(mut self, type_name: impl Into<String>) -> Self {
        self.type_name = Some(type_name.into());
        self
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub type_name: String,
}

impl Parameter {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
        }
    }
}

/// A declared or implemented method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Method {
    pub id: SymbolId,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub generics: Vec<GenericParam>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub location: SourceLocation,
}

impl Method {
    pub fn new(id: SymbolId, name: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            id,
            name: name.into(),
            parameters: Vec::new(),
            return_type: None,
            generics: Vec::new(),
            visibility: Visibility::Private,
            is_static: false,
            location,
        }
    }

    pub fn with_parameters(mut self, parameters: impl IntoIterator<Item = Parameter>) -> Self {
        self.parameters = parameters.into_iter().collect();
        self
    }

    pub fn with_return_type(mut self, return_type: impl Into<String>) -> Self {
        self.return_type = Some(return_type.into());
        self
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_generics(mut self, generics: impl IntoIterator<Item = GenericParam>) -> Self {
        self.generics = generics.into_iter().collect();
        self
    }

    pub fn with_static(mut self, is_static: bool) -> Self {
        self.is_static = is_static;
        self
    }
}

/// An enum variant. Tuple variants use field names `0`, `1`, and so on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub id: SymbolId,
    pub name: String,
    pub fields: Vec<Field>,
    pub location: SourceLocation,
}

impl Variant {
    pub fn new(id: SymbolId, name: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            id,
            name: name.into(),
            fields: Vec::new(),
            location,
        }
    }
}

/// A top-level type declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDecl {
    pub id: SymbolId,
    pub name: String,
    pub qualified_name: String,
    pub kind: TypeKind,
    /// RHS type text for a type alias; `None` for other declarations.
    pub alias_of: Option<String>,
    pub module: Option<SymbolId>,
    pub visibility: Visibility,
    pub generics: Vec<GenericParam>,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
    pub variants: Vec<Variant>,
    pub location: SourceLocation,
}

impl TypeDecl {
    pub fn new(
        id: SymbolId,
        name: impl Into<String>,
        qualified_name: impl Into<String>,
        kind: TypeKind,
        location: SourceLocation,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            qualified_name: qualified_name.into(),
            kind,
            alias_of: None,
            module: None,
            visibility: Visibility::Private,
            generics: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            variants: Vec::new(),
            location,
        }
    }

    pub fn with_module(mut self, module: SymbolId) -> Self {
        self.module = Some(module);
        self
    }

    pub fn with_alias_of(mut self, alias_of: impl Into<String>) -> Self {
        self.alias_of = Some(alias_of.into());
        self
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_generics(mut self, generics: impl IntoIterator<Item = GenericParam>) -> Self {
        self.generics = generics.into_iter().collect();
        self
    }
}
