//! Turning extracted facts into modules and type declarations in the shared model.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codeuml_core::{
    Codebase, Field, Method, Module, Parameter, SourceLocation, SymbolId, TypeDecl, TypeKind,
    Variant,
};

use crate::discovery::PySource;
use crate::facts::{ClassFact, ClassKind, FieldFact, FileFacts, MethodFact};

const UNKNOWN_TYPE: &str = "<unknown>";

/// A parsed file together with the module it defines.
#[derive(Debug)]
pub(crate) struct AnalyzedFile {
    pub source: PySource,
    pub module: String,
    pub facts: FileFacts,
}

/// A class that made it into the model.
#[derive(Clone, Debug)]
pub(crate) struct DeclaredClass {
    pub file: usize,
    pub class: usize,
    pub id: SymbolId,
    pub qualified: String,
    pub kind: ClassKind,
}

struct ModuleInfo {
    name: String,
    file: PathBuf,
    parent: Option<String>,
}

/// Packages (directories) and modules (files), keyed by dotted name. A package
/// is represented by its `__init__.py` when it has one, otherwise by its
/// directory.
fn module_infos(sources: &[PySource]) -> BTreeMap<String, ModuleInfo> {
    let mut infos = BTreeMap::new();
    for source in sources {
        add_package_infos(&mut infos, source);
        if !source.is_init() {
            infos.entry(source.module_name()).or_insert(ModuleInfo {
                name: source.stem.clone(),
                file: source.path.clone(),
                parent: (!source.dirs.is_empty()).then(|| source.package_name()),
            });
        }
    }
    infos
}

fn add_package_infos(infos: &mut BTreeMap<String, ModuleInfo>, source: &PySource) {
    let depth = source.dirs.len();
    for level in 1..=depth {
        let is_this_package = source.is_init() && level == depth;
        let file = if is_this_package {
            source.path.clone()
        } else {
            source
                .dir_path
                .ancestors()
                .nth(depth - level)
                .unwrap_or(&source.dir_path)
                .to_path_buf()
        };
        let info = ModuleInfo {
            name: source.dirs[level - 1].clone(),
            file,
            parent: (level > 1).then(|| source.dirs[..level - 1].join(".")),
        };
        let qualified = source.dirs[..level].join(".");
        if is_this_package {
            infos.insert(qualified, info);
        } else {
            infos.entry(qualified).or_insert(info);
        }
    }
}

fn top_of_file(file: &Path) -> SourceLocation {
    SourceLocation::new(file, 1, 1, 1, 1)
}

/// Add every package and module; returns their IDs by dotted name.
pub(crate) fn add_modules(
    codebase: &mut Codebase,
    sources: &[PySource],
    errors: &mut Vec<String>,
) -> BTreeMap<String, SymbolId> {
    let infos = module_infos(sources);
    let ids: BTreeMap<String, SymbolId> = infos
        .iter()
        .map(|(qualified, info)| {
            (
                qualified.clone(),
                SymbolId::declaration(&info.file, qualified),
            )
        })
        .collect();
    for (qualified, info) in &infos {
        let mut module = Module::new(
            ids[qualified].clone(),
            &info.name,
            qualified,
            &info.file,
            top_of_file(&info.file),
        );
        if let Some(parent) = info.parent.as_ref().and_then(|parent| ids.get(parent)) {
            module = module.with_parent(parent.clone());
        }
        if let Err(error) = codebase.add_module(module) {
            errors.push(format!("could not add module: {error}"));
        }
    }
    ids
}

fn type_kind(kind: ClassKind) -> TypeKind {
    match kind {
        ClassKind::Class => TypeKind::Class,
        ClassKind::Interface => TypeKind::Interface,
        ClassKind::Enum => TypeKind::Enum,
    }
}

fn field(owner: &SymbolId, fact: &FieldFact) -> Field {
    let field = Field::new(
        SymbolId::field(owner, &fact.name),
        &fact.name,
        fact.location.clone(),
    )
    .with_visibility(fact.visibility.clone());
    match &fact.annotation {
        Some(annotation) => field.with_type(&annotation.text),
        None => field,
    }
}

fn method(owner: &SymbolId, fact: &MethodFact) -> Method {
    let parameters = fact.params.iter().map(|param| {
        let type_name = param
            .annotation
            .as_ref()
            .map_or(UNKNOWN_TYPE, |annotation| annotation.text.as_str());
        Parameter::new(&param.name, type_name)
    });
    let method = Method::new(
        SymbolId::method(owner, &fact.name),
        &fact.name,
        fact.location.clone(),
    )
    .with_parameters(parameters)
    .with_visibility(fact.visibility.clone())
    .with_static(fact.is_static);
    match &fact.returns {
        Some(returns) => method.with_return_type(&returns.text),
        None => method,
    }
}

fn type_decl(
    class: &ClassFact,
    id: SymbolId,
    qualified: String,
    module: Option<&SymbolId>,
) -> TypeDecl {
    let mut declaration = TypeDecl::new(
        id.clone(),
        &class.name,
        qualified,
        type_kind(class.kind),
        class.location.clone(),
    )
    .with_visibility(class.visibility.clone());
    if let Some(module) = module {
        declaration = declaration.with_module(module.clone());
    }
    declaration.fields = class.fields.iter().map(|fact| field(&id, fact)).collect();
    declaration.methods = class.methods.iter().map(|fact| method(&id, fact)).collect();
    declaration.variants = class
        .variants
        .iter()
        .map(|(name, location)| Variant::new(SymbolId::variant(&id, name), name, location.clone()))
        .collect();
    declaration
}

/// Add every class; returns the ones the model accepted.
pub(crate) fn add_types(
    codebase: &mut Codebase,
    files: &[AnalyzedFile],
    modules: &BTreeMap<String, SymbolId>,
    errors: &mut Vec<String>,
) -> Vec<DeclaredClass> {
    let mut declared = Vec::new();
    for (file_index, file) in files.iter().enumerate() {
        for (class_index, class) in file.facts.classes.iter().enumerate() {
            let qualified = format!("{}.{}", file.module, class.local_name());
            let id = SymbolId::declaration(&file.source.path, &qualified);
            let declaration = type_decl(
                class,
                id.clone(),
                qualified.clone(),
                modules.get(&file.module),
            );
            match codebase.add_type(declaration) {
                Ok(()) => declared.push(DeclaredClass {
                    file: file_index,
                    class: class_index,
                    id,
                    qualified,
                    kind: class.kind,
                }),
                Err(error) => errors.push(format!("could not add type declaration: {error}")),
            }
        }
    }
    declared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract;

    fn source(path: &str, dirs: &[&str], stem: &str) -> PySource {
        let path = PathBuf::from(path);
        PySource {
            dir_path: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            path,
            dirs: dirs.iter().map(|dir| (*dir).to_owned()).collect(),
            stem: stem.to_owned(),
        }
    }

    #[test]
    fn packages_use_their_init_file_or_their_directory() {
        let sources = [
            source("pkg/__init__.py", &["pkg"], "__init__"),
            source("pkg/a.py", &["pkg"], "a"),
            source("loose/sub/b.py", &["loose", "sub"], "b"),
            source("top.py", &[], "top"),
        ];
        let infos = module_infos(&sources);
        let describe = |name: &str| {
            let info = &infos[name];
            (
                info.file.to_string_lossy().replace('\\', "/"),
                info.parent.clone(),
            )
        };
        assert_eq!(describe("pkg"), ("pkg/__init__.py".to_owned(), None));
        assert_eq!(
            describe("pkg.a"),
            ("pkg/a.py".to_owned(), Some("pkg".to_owned()))
        );
        assert_eq!(describe("loose"), ("loose".to_owned(), None));
        assert_eq!(
            describe("loose.sub"),
            ("loose/sub".to_owned(), Some("loose".to_owned()))
        );
        assert_eq!(
            describe("loose.sub.b"),
            ("loose/sub/b.py".to_owned(), Some("loose.sub".to_owned()))
        );
        assert_eq!(describe("top"), ("top.py".to_owned(), None));
        assert_eq!(infos.len(), 6);
    }

    #[test]
    fn an_init_file_wins_whichever_order_files_arrive_in() {
        let sources = [
            source("pkg/a.py", &["pkg"], "a"),
            source("pkg/__init__.py", &["pkg"], "__init__"),
        ];
        assert_eq!(
            module_infos(&sources)["pkg"].file,
            PathBuf::from("pkg/__init__.py")
        );
    }

    fn analyzed(path: &str, dirs: &[&str], stem: &str, code: &str) -> AnalyzedFile {
        let source = source(path, dirs, stem);
        let facts = extract(code, &source.path).expect("valid python");
        AnalyzedFile {
            module: source.module_name(),
            source,
            facts,
        }
    }

    #[test]
    fn modules_types_and_members_reach_the_model() {
        let code = "class Shape:\n    sides: int = 0\n    _hidden = 1\n\n    def area(self, scale: float, *rest) -> float: ...\n    @staticmethod\n    def make() -> 'Shape': ...\n\nclass Color(Enum):\n    RED = 1\n\nclass Reader(Protocol): ...\n";
        let files = [analyzed("geo/shapes.py", &["geo"], "shapes", code)];
        let mut codebase = Codebase::new("p");
        let mut errors = Vec::new();
        let sources = [files[0].source.clone()];

        let modules = add_modules(&mut codebase, &sources, &mut errors);
        let declared = add_types(&mut codebase, &files, &modules, &mut errors);

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(codebase.modules().len(), 2);
        assert_eq!(declared.len(), 3);
        let shape = codebase
            .types()
            .iter()
            .find(|decl| decl.qualified_name == "geo.shapes.Shape")
            .expect("shape");
        assert_eq!(shape.kind, TypeKind::Class);
        assert_eq!(shape.module.as_ref(), modules.get("geo.shapes"));
        assert_eq!(shape.fields.len(), 2);
        assert_eq!(shape.fields[0].type_name.as_deref(), Some("int"));
        assert!(shape.fields[1].type_name.is_none());
        let area = &shape.methods[0];
        assert_eq!(area.parameters.len(), 2);
        assert_eq!(area.parameters[0].type_name, "float");
        assert_eq!(area.parameters[1].type_name, UNKNOWN_TYPE);
        assert_eq!(area.return_type.as_deref(), Some("float"));
        assert!(!area.is_static && shape.methods[1].is_static);
        let kinds: Vec<_> = codebase
            .types()
            .iter()
            .map(|decl| decl.kind.clone())
            .collect();
        assert_eq!(
            kinds,
            [TypeKind::Class, TypeKind::Enum, TypeKind::Interface]
        );
        assert_eq!(codebase.types()[1].variants.len(), 1);
    }

    #[test]
    fn duplicate_models_are_reported_without_aborting() {
        let files = [analyzed("a.py", &[], "a", "class A: ...\n")];
        let mut codebase = Codebase::new("p");
        let mut errors = Vec::new();
        let sources = [files[0].source.clone()];
        let modules = add_modules(&mut codebase, &sources, &mut errors);
        add_types(&mut codebase, &files, &modules, &mut errors);
        assert!(errors.is_empty());

        // Adding the same files again collides on every ID.
        let modules = add_modules(&mut codebase, &sources, &mut errors);
        let declared = add_types(&mut codebase, &files, &modules, &mut errors);
        assert!(declared.is_empty());
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("could not add module"));
        assert!(errors[1].contains("could not add type declaration"));
    }
}
