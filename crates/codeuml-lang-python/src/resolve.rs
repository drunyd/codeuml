//! Resolving the names used in annotations and base classes to declared classes.
//!
//! Resolution is syntactic and conservative, like the Rust provider's. A name
//! is followed through the file's imports (including relative imports and
//! `__init__` re-exports) to a class in the project; anything that leads
//! outside the project, or into the builtins, is external and creates no
//! relationship. Names that look project-internal but cannot be found stay in
//! the model as unresolved relationships.

use std::collections::{BTreeSet, HashMap, HashSet};

use codeuml_core::{
    Codebase, Diagnostic, DiagnosticSeverity, RelationshipEvidence, RelationshipKind, SymbolId,
};
use codeuml_core::{Relationship, SourceLocation};

use crate::build::{AnalyzedFile, DeclaredClass};
use crate::facts::{ClassKind, Import, NameRef};

/// Re-exports are followed this many hops before giving up.
const MAX_HOPS: usize = 8;

const BUILTINS: &[&str] = &[
    "int",
    "float",
    "str",
    "bool",
    "bytes",
    "bytearray",
    "complex",
    "list",
    "dict",
    "set",
    "frozenset",
    "tuple",
    "type",
    "object",
    "range",
    "slice",
    "memoryview",
    "Exception",
    "BaseException",
];

#[derive(Debug, PartialEq, Eq)]
enum Binding {
    /// A name bound to a module (`import a.b as c`).
    Module(String),
    /// A name imported from a module (`from m import name`).
    Symbol { module: String, name: String },
    /// Known to be external or not a class (type variables, unreachable imports).
    Ignored,
}

#[derive(Debug, Default)]
struct Scope {
    bindings: HashMap<String, Binding>,
    /// Modules named by `from m import *`.
    stars: Vec<String>,
    /// Module-level names bound by functions and assignments (not classes).
    defined: HashSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Index into the declared classes.
    Type(usize),
    External,
    Unresolved {
        internal: bool,
    },
    Ambiguous(Vec<String>),
}

fn join(base: &str, name: &str) -> String {
    match (base.is_empty(), name.is_empty()) {
        (true, _) => name.to_owned(),
        (false, true) => base.to_owned(),
        (false, false) => format!("{base}.{name}"),
    }
}

/// The module a `from` import starts at, given the importing file's package.
fn import_base(level: usize, module: &[String], package: &str) -> Option<String> {
    if level == 0 {
        return Some(module.join("."));
    }
    let segments: Vec<&str> = package.split('.').filter(|part| !part.is_empty()).collect();
    let keep = segments.len().checked_sub(level - 1)?;
    // Climbing out of the top-level package is an error in Python.
    if keep == 0 && !segments.is_empty() {
        return None;
    }
    Some(join(&segments[..keep].join("."), &module.join(".")))
}

fn scope_for(file: &AnalyzedFile) -> Scope {
    let mut scope = Scope::default();
    let package = file.source.package_name();
    for import in &file.facts.imports {
        bind_import(&mut scope, import, &package);
    }
    for name in &file.facts.type_vars {
        scope.bindings.insert(name.clone(), Binding::Ignored);
    }
    scope.defined = file.facts.defined.iter().cloned().collect();
    scope
}

fn bind_import(scope: &mut Scope, import: &Import, package: &str) {
    match import {
        Import::Module { path, alias } => bind_module(scope, path, alias.as_deref()),
        Import::From {
            level,
            module,
            names,
            wildcard,
        } => {
            let base = import_base(*level, module, package);
            if let (true, Some(base)) = (*wildcard, base.as_ref()) {
                scope.stars.push(base.clone());
            }
            for imported in names {
                let key = imported.alias.as_ref().unwrap_or(&imported.name).clone();
                let binding = match &base {
                    Some(module) => Binding::Symbol {
                        module: module.clone(),
                        name: imported.name.clone(),
                    },
                    None => Binding::Ignored,
                };
                scope.bindings.insert(key, binding);
            }
        }
    }
}

/// `import a.b` binds `a`; `import a.b as c` binds `c` to `a.b`.
fn bind_module(scope: &mut Scope, path: &[String], alias: Option<&str>) {
    let (key, target) = match alias {
        Some(alias) => (alias, path.join(".")),
        None => match path.first() {
            Some(first) => (first.as_str(), first.clone()),
            None => return,
        },
    };
    scope
        .bindings
        .insert(key.to_owned(), Binding::Module(target));
}

pub(crate) struct Resolver<'a> {
    declared: &'a [DeclaredClass],
    classes: HashMap<&'a str, Vec<usize>>,
    modules: &'a BTreeSet<String>,
    tops: BTreeSet<&'a str>,
    scopes: HashMap<&'a str, Scope>,
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(
        files: &'a [AnalyzedFile],
        declared: &'a [DeclaredClass],
        modules: &'a BTreeSet<String>,
    ) -> Self {
        let mut classes: HashMap<&str, Vec<usize>> = HashMap::new();
        for (index, class) in declared.iter().enumerate() {
            classes
                .entry(class.qualified.as_str())
                .or_default()
                .push(index);
        }
        Self {
            declared,
            classes,
            modules,
            tops: modules
                .iter()
                .filter_map(|name| name.split('.').next())
                .collect(),
            scopes: files
                .iter()
                .map(|file| (file.module.as_str(), scope_for(file)))
                .collect(),
        }
    }

    fn is_internal(&self, module: &str) -> bool {
        module.is_empty()
            || module
                .split('.')
                .next()
                .is_some_and(|top| self.tops.contains(top))
    }

    fn lookup(&self, qualified: &str) -> Option<Outcome> {
        let found = self.classes.get(qualified)?;
        Some(match found.as_slice() {
            [only] => Outcome::Type(*only),
            many => Outcome::Ambiguous(
                many.iter()
                    .map(|index| self.declared[*index].qualified.clone())
                    .collect(),
            ),
        })
    }

    /// A name that is absent from a module: unresolved inside the project,
    /// external outside it.
    fn missing(&self, module: &str) -> Outcome {
        if self.is_internal(module) {
            Outcome::Unresolved { internal: true }
        } else {
            Outcome::External
        }
    }

    pub(crate) fn resolve(&self, module: &str, path: &[String]) -> Outcome {
        let Some(first) = path.first() else {
            return Outcome::External;
        };
        if let Some(local) = self.lookup(&join(module, &path.join("."))) {
            return local;
        }
        let binding = self
            .scopes
            .get(module)
            .and_then(|scope| scope.bindings.get(first));
        match binding {
            Some(Binding::Ignored) => Outcome::External,
            Some(Binding::Module(target)) => self.in_module(target, &path[1..], 0),
            Some(Binding::Symbol { module: from, name }) => {
                self.imported_symbol(from, name, &path[1..], 0)
            }
            None => self.unbound(module, path),
        }
    }

    /// `rest` names something inside `module`; the last segment is the class.
    fn in_module(&self, module: &str, rest: &[String], hops: usize) -> Outcome {
        let Some((name, parents)) = rest.split_last() else {
            return Outcome::External;
        };
        if let Some(found) = self.lookup(&join(module, &rest.join("."))) {
            return found;
        }
        self.follow(&join(module, &parents.join(".")), name, hops)
    }

    /// `from module import name` followed by `rest` attribute accesses.
    fn imported_symbol(&self, module: &str, name: &str, rest: &[String], hops: usize) -> Outcome {
        let submodule = join(module, name);
        if self.modules.contains(&submodule) {
            return self.in_module(&submodule, rest, hops);
        }
        if let Some(found) = self.lookup(&join(&submodule, &rest.join("."))) {
            return found;
        }
        if rest.is_empty() {
            self.follow(module, name, hops)
        } else {
            self.missing(module)
        }
    }

    /// `name` is not defined as a class in `module`; it may be re-exported by
    /// its imports, be a non-class name, or really be missing.
    fn follow(&self, module: &str, name: &str, hops: usize) -> Outcome {
        if hops >= MAX_HOPS {
            return Outcome::Unresolved { internal: true };
        }
        let binding = self
            .scopes
            .get(module)
            .and_then(|scope| scope.bindings.get(name));
        match binding {
            Some(Binding::Symbol { module: from, name }) => {
                self.imported_symbol(from, name, &[], hops + 1)
            }
            Some(_) => Outcome::External,
            None => self.absent(module, name, hops),
        }
    }

    /// A name with no import binding in `module`: defined there as something
    /// other than a class, provided by a star import, or missing.
    fn absent(&self, module: &str, name: &str, hops: usize) -> Outcome {
        let Some(scope) = self.scopes.get(module) else {
            return self.missing(module);
        };
        if scope.defined.contains(name) {
            return Outcome::External;
        }
        for star in &scope.stars {
            let found = self.imported_symbol(star, name, &[], hops + 1);
            if matches!(found, Outcome::Type(_) | Outcome::Ambiguous(_)) {
                return found;
            }
        }
        if scope.stars.iter().any(|star| !self.is_internal(star)) {
            Outcome::External
        } else {
            self.missing(module)
        }
    }

    /// A first segment with no import binding: a name defined here that is not
    /// a class, a builtin, a star import, or unknown.
    fn unbound(&self, module: &str, path: &[String]) -> Outcome {
        let first = &path[0];
        let scope = self.scopes.get(module);
        if BUILTINS.contains(&first.as_str())
            || scope.is_some_and(|scope| scope.defined.contains(first))
        {
            return Outcome::External;
        }
        let stars = scope.map_or(&[][..], |scope| &scope.stars[..]);
        for star in stars {
            let found = self.imported_symbol(star, first, &path[1..], 0);
            if matches!(found, Outcome::Type(_) | Outcome::Ambiguous(_)) {
                return found;
            }
        }
        if stars.iter().any(|star| !self.is_internal(star)) {
            Outcome::External
        } else {
            Outcome::Unresolved { internal: false }
        }
    }
}

/// An `Extends` edge to an interface is an `Implements` edge.
fn relation_kind(kind: RelationshipKind, target: ClassKind) -> RelationshipKind {
    if kind == RelationshipKind::Extends && target == ClassKind::Interface {
        RelationshipKind::Implements
    } else {
        kind
    }
}

fn warning(code: &str, message: String, location: &SourceLocation) -> Diagnostic {
    Diagnostic::new(DiagnosticSeverity::Warning, message, Some(location.clone())).with_code(code)
}

/// Adds relationships for every declared class and collects diagnostics.
pub(crate) struct Linker<'a, 'm> {
    pub resolver: &'a Resolver<'a>,
    pub codebase: &'m mut Codebase,
    pub diagnostics: Vec<Diagnostic>,
}

impl Linker<'_, '_> {
    pub(crate) fn link_class(&mut self, files: &[AnalyzedFile], entry: &DeclaredClass) {
        let file = &files[entry.file];
        let class = &file.facts.classes[entry.class];
        for base in &class.bases {
            self.link(&entry.id, base, RelationshipKind::Extends, &file.module);
        }
        for field in &class.fields {
            let source = SymbolId::field(&entry.id, &field.name);
            let refs = field
                .annotation
                .iter()
                .flat_map(|annotation| &annotation.refs);
            self.link_all(&source, refs, RelationshipKind::FieldType, &file.module);
        }
        for method in &class.methods {
            let source = SymbolId::method(&entry.id, &method.name);
            let params = method
                .params
                .iter()
                .filter_map(|param| param.annotation.as_ref())
                .flat_map(|annotation| &annotation.refs);
            self.link_all(
                &source,
                params,
                RelationshipKind::MethodParameterType,
                &file.module,
            );
            let returns = method
                .returns
                .iter()
                .flat_map(|annotation| &annotation.refs);
            self.link_all(
                &source,
                returns,
                RelationshipKind::MethodReturnType,
                &file.module,
            );
        }
    }

    fn link_all<'r>(
        &mut self,
        source: &SymbolId,
        refs: impl Iterator<Item = &'r NameRef>,
        kind: RelationshipKind,
        module: &str,
    ) {
        for reference in refs {
            self.link(source, reference, kind, module);
        }
    }

    fn link(
        &mut self,
        source: &SymbolId,
        reference: &NameRef,
        kind: RelationshipKind,
        module: &str,
    ) {
        let name = reference.display();
        let relationship = match self.resolver.resolve(module, &reference.path) {
            Outcome::External => return,
            Outcome::Type(index) => {
                let target = &self.resolver.declared[index];
                Relationship::inferred(source, &target.id, relation_kind(kind, target.kind))
            }
            Outcome::Unresolved { internal } => {
                if internal {
                    self.diagnostics.push(warning(
                        "analysis::unresolved-reference",
                        format!("unresolved local reference `{name}`"),
                        &reference.location,
                    ));
                }
                Relationship::unresolved(source, &name, kind)
            }
            Outcome::Ambiguous(candidates) => {
                self.diagnostics.push(warning(
                    "analysis::ambiguous-reference",
                    format!(
                        "ambiguous reference `{name}`; candidates: {}",
                        candidates.join(", ")
                    ),
                    &reference.location,
                ));
                Relationship::unresolved(source, &name, kind)
            }
        };
        let evidence = RelationshipEvidence::new(reference.location.clone()).with_note(name);
        if let Err(error) = self
            .codebase
            .add_relationship(relationship.with_evidence(evidence))
        {
            self.diagnostics.push(
                Diagnostic::error(
                    format!("could not add resolved reference: {error}"),
                    Some(reference.location.clone()),
                )
                .with_code("analysis::model-invariant"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{add_modules, add_types};
    use crate::test_support::analyzed_file;

    struct World {
        files: Vec<AnalyzedFile>,
        declared: Vec<DeclaredClass>,
        modules: BTreeSet<String>,
    }

    impl World {
        fn new(files: Vec<AnalyzedFile>) -> Self {
            let mut codebase = Codebase::new("world");
            let mut errors = Vec::new();
            let sources: Vec<_> = files.iter().map(|file| file.source.clone()).collect();
            let ids = add_modules(&mut codebase, &sources, &mut errors);
            let declared = add_types(&mut codebase, &files, &ids, &mut errors);
            assert!(errors.is_empty(), "{errors:?}");
            Self {
                files,
                declared,
                modules: ids.keys().cloned().collect(),
            }
        }

        fn from_sources(sources: &[(&str, &str)]) -> Self {
            Self::new(
                sources
                    .iter()
                    .map(|(path, code)| analyzed_file(path, code))
                    .collect(),
            )
        }

        /// What `name` (dotted) resolves to when written inside `module`.
        fn outcome(&self, module: &str, name: &str) -> String {
            let resolver = Resolver::new(&self.files, &self.declared, &self.modules);
            let path: Vec<String> = name
                .split('.')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect();
            match resolver.resolve(module, &path) {
                Outcome::Type(index) => self.declared[index].qualified.clone(),
                Outcome::External => "external".to_owned(),
                Outcome::Unresolved { internal: true } => "missing-internal".to_owned(),
                Outcome::Unresolved { internal: false } => "unknown".to_owned(),
                Outcome::Ambiguous(candidates) => format!("ambiguous:{}", candidates.join("|")),
            }
        }
    }

    fn shop() -> World {
        World::from_sources(&[
            (
                "shop/__init__.py",
                "from .models import Customer, Order as PurchaseOrder\nfrom . import billing\n",
            ),
            (
                "shop/models.py",
                "class Customer: ...\nclass Order: ...\nclass Box:\n    class Inner: ...\n",
            ),
            (
                "shop/billing.py",
                "import numpy as np\nimport shop.models as m\nimport shop.models\nfrom . import models\nfrom .models import Customer\nfrom .missing import Ghost\nfrom shop.models import Absent\nfrom typing import Optional\nT = TypeVar('T')\nclass Invoice: ...\nclass Customer: ...\n",
            ),
            (
                "shop/util.py",
                "from .models import *\nfrom os.path import *\nclass Helper: ...\n",
            ),
            ("shop/only_star.py", "from .models import *\n"),
            ("shop/cycle_a.py", "from .cycle_b import Loop\n"),
            ("shop/cycle_b.py", "from .cycle_a import Loop\n"),
            (
                "app.py",
                "import shop\nfrom shop import Customer, PurchaseOrder, billing\nfrom shop.billing import Invoice\nfrom .. import beyond\n",
            ),
            ("peer.py", "class P: ...\n"),
            (
                "shop/types.py",
                "from typing import Any\nJson = dict[str, Any]\ntype Alias = list[int]\ndef helper(): ...\nclass Real: ...\n",
            ),
            ("shop/reexp.py", "from .models import *\n"),
            (
                "shop/user.py",
                "from .types import Json, Alias, helper, Real, Nope\nfrom .reexp import Customer, Unknown\nclass U: ...\n",
            ),
            ("top.py", "from . import peer\n"),
        ])
    }

    #[test]
    fn local_nested_and_shadowing_classes_resolve_without_imports() {
        let world = shop();
        assert_eq!(
            world.outcome("shop.billing", "Invoice"),
            "shop.billing.Invoice"
        );
        // A class defined here shadows the imported one of the same name.
        assert_eq!(
            world.outcome("shop.billing", "Customer"),
            "shop.billing.Customer"
        );
        assert_eq!(
            world.outcome("shop.models", "Box.Inner"),
            "shop.models.Box.Inner"
        );
        assert_eq!(world.outcome("shop.models", ""), "external");
    }

    #[test]
    fn module_imports_resolve_through_aliases_and_dotted_paths() {
        let world = shop();
        assert_eq!(
            world.outcome("shop.billing", "m.Order"),
            "shop.models.Order"
        );
        assert_eq!(
            world.outcome("shop.billing", "shop.models.Order"),
            "shop.models.Order"
        );
        assert_eq!(
            world.outcome("app", "shop.billing.Invoice"),
            "shop.billing.Invoice"
        );
        assert_eq!(
            world.outcome("shop.billing", "models.Box.Inner"),
            "shop.models.Box.Inner"
        );
        assert_eq!(
            world.outcome("app", "billing.Invoice"),
            "shop.billing.Invoice"
        );
        assert_eq!(world.outcome("app", "Invoice"), "shop.billing.Invoice");
        // A module is not a type.
        assert_eq!(world.outcome("app", "shop"), "external");
    }

    #[test]
    fn package_re_exports_are_followed_including_renames() {
        let world = shop();
        assert_eq!(world.outcome("app", "Customer"), "shop.models.Customer");
        assert_eq!(world.outcome("app", "PurchaseOrder"), "shop.models.Order");
        assert_eq!(
            world.outcome("app", "shop.Customer"),
            "shop.models.Customer"
        );
        assert_eq!(
            world.outcome("app", "shop.PurchaseOrder"),
            "shop.models.Order"
        );
    }

    #[test]
    fn relative_imports_resolve_from_the_package_and_stop_at_the_root() {
        let world = shop();
        assert_eq!(world.outcome("top", "peer.P"), "peer.P");
        assert_eq!(world.outcome("app", "beyond"), "external");
    }

    #[test]
    fn external_libraries_builtins_and_type_variables_are_not_project_classes() {
        let world = shop();
        assert_eq!(world.outcome("shop.billing", "np.ndarray"), "external");
        assert_eq!(world.outcome("shop.billing", "Optional"), "external");
        assert_eq!(world.outcome("shop.billing", "T"), "external");
        assert_eq!(world.outcome("shop.billing", "int"), "external");
        assert_eq!(world.outcome("shop.billing", "str"), "external");
    }

    #[test]
    fn project_internal_names_that_cannot_be_found_are_unresolved() {
        let world = shop();
        assert_eq!(world.outcome("shop.billing", "Ghost"), "missing-internal");
        assert_eq!(world.outcome("shop.billing", "Absent"), "missing-internal");
        assert_eq!(world.outcome("app", "Customer.Nested"), "missing-internal");
        assert_eq!(world.outcome("shop.billing", "Whatever"), "unknown");
    }

    #[test]
    fn star_imports_resolve_names_from_internal_modules_only() {
        let world = shop();
        assert_eq!(
            world.outcome("shop.util", "Customer"),
            "shop.models.Customer"
        );
        // `from os.path import *` could supply anything.
        assert_eq!(world.outcome("shop.util", "Nope"), "external");
        assert_eq!(world.outcome("shop.only_star", "Nope"), "unknown");
        assert_eq!(
            world.outcome("shop.only_star", "Order"),
            "shop.models.Order"
        );
    }

    #[test]
    fn import_cycles_terminate() {
        let world = shop();
        assert_eq!(world.outcome("shop.cycle_a", "Loop"), "missing-internal");
    }

    #[test]
    fn names_that_exist_but_are_not_classes_are_not_reported_as_missing() {
        let world = shop();
        // Aliases and functions defined in a project module are real names.
        assert_eq!(world.outcome("shop.user", "Json"), "external");
        assert_eq!(world.outcome("shop.user", "Alias"), "external");
        assert_eq!(world.outcome("shop.user", "helper"), "external");
        assert_eq!(world.outcome("shop.types", "Json"), "external");
        assert_eq!(world.outcome("shop.types", "helper"), "external");
        // A class from the same module still resolves, and a name the module
        // lacks entirely is still missing.
        assert_eq!(world.outcome("shop.user", "Real"), "shop.types.Real");
        assert_eq!(world.outcome("shop.user", "Nope"), "missing-internal");
    }

    #[test]
    fn star_imports_re_export_classes_to_importers() {
        let world = shop();
        assert_eq!(
            world.outcome("shop.user", "Customer"),
            "shop.models.Customer"
        );
        assert_eq!(world.outcome("shop.user", "Unknown"), "missing-internal");
    }

    #[test]
    fn classes_defined_twice_under_one_module_name_are_ambiguous() {
        let mut first = analyzed_file("one/dup.py", "class Dup: ...\n");
        let mut second = analyzed_file("two/dup.py", "class Dup: ...\nclass User(Dup): ...\n");
        first.module = "dup".to_owned();
        second.module = "dup".to_owned();
        let world = World::new(vec![first, second]);
        assert_eq!(world.outcome("dup", "Dup"), "ambiguous:dup.Dup|dup.Dup");
    }

    #[test]
    fn imports_resolve_against_the_importing_files_package() {
        assert_eq!(
            import_base(0, &["a".into(), "b".into()], "x.y").as_deref(),
            Some("a.b")
        );
        assert_eq!(
            import_base(1, &["m".into()], "pkg.sub").as_deref(),
            Some("pkg.sub.m")
        );
        assert_eq!(
            import_base(2, &["m".into()], "pkg.sub").as_deref(),
            Some("pkg.m")
        );
        assert_eq!(import_base(2, &[], "pkg.sub").as_deref(), Some("pkg"));
        assert_eq!(import_base(1, &[], "").as_deref(), Some(""));
        assert_eq!(import_base(3, &[], "pkg.sub"), None);
        assert_eq!(join("", "a"), "a");
        assert_eq!(join("a", ""), "a");
        assert_eq!(join("a", "b"), "a.b");
    }

    #[test]
    fn interface_bases_make_an_implements_edge() {
        assert_eq!(
            relation_kind(RelationshipKind::Extends, ClassKind::Interface),
            RelationshipKind::Implements
        );
        assert_eq!(
            relation_kind(RelationshipKind::Extends, ClassKind::Class),
            RelationshipKind::Extends
        );
        assert_eq!(
            relation_kind(RelationshipKind::FieldType, ClassKind::Interface),
            RelationshipKind::FieldType
        );
    }

    #[test]
    fn a_bare_module_import_binds_its_first_segment_and_empty_imports_bind_nothing() {
        let mut scope = Scope::default();
        bind_module(&mut scope, &["a".to_owned(), "b".to_owned()], None);
        bind_module(&mut scope, &["a".to_owned(), "b".to_owned()], Some("c"));
        bind_module(&mut scope, &[], None);
        assert_eq!(scope.bindings["a"], Binding::Module("a".to_owned()));
        assert_eq!(scope.bindings["c"], Binding::Module("a.b".to_owned()));
        assert_eq!(scope.bindings.len(), 2);
    }
}
