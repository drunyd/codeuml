use std::fmt;
use std::path::Path;

/// Stable identity for a declaration or a member declaration.
///
/// IDs are generated from project-relative paths and qualified declaration
/// names so that identical short names in different modules remain distinct.
/// The format is intentionally opaque; consumers should only compare and
/// display IDs through this type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(String);

impl SymbolId {
    /// Build an ID for a declaration at a project-relative path.
    ///
    /// Path separators are normalized so IDs stay stable across platforms.
    pub fn declaration(path: impl AsRef<Path>, qualified_name: &str) -> Self {
        debug_assert!(!qualified_name.is_empty());
        let path = path.as_ref().to_string_lossy().replace('\\', "/");
        Self(format!("{path}#{qualified_name}"))
    }

    /// Build an ID for a member of another declaration, such as a field or
    /// method. The parent ID keeps members globally unique.
    pub fn member(parent: &Self, name: &str) -> Self {
        debug_assert!(!name.is_empty());
        Self(format!("{parent}::{name}"))
    }

    /// Build an ID for a struct/enum field. Namespacing prevents a field and a
    /// method with the same display name from sharing an ID.
    pub fn field(parent: &Self, name: &str) -> Self {
        Self::member(parent, &format!("field:{name}"))
    }

    /// Build an ID for a method or associated function.
    pub fn method(parent: &Self, name: &str) -> Self {
        Self::member(parent, &format!("method:{name}"))
    }

    /// Build a method ID qualified by an owning trait. This distinguishes an
    /// inherent method from a trait implementation method with the same name.
    pub fn method_in(parent: &Self, qualifier: &str, name: &str) -> Self {
        Self::member(parent, &format!("method:{qualifier}::{name}"))
    }

    /// Build an ID for an enum variant.
    pub fn variant(parent: &Self, name: &str) -> Self {
        Self::member(parent, &format!("variant:{name}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for SymbolId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
