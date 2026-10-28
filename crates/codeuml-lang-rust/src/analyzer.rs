use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use codeuml_core::{
    Diagnostic, DiagnosticSeverity, Field, GenericParam, Method, Module, Parameter,
    RelationshipKind, SourceLocation, SymbolId, TypeDecl, TypeKind, Variant, Visibility,
};
use proc_macro2::Span;
use quote::ToTokens;
use syn::spanned::Spanned;

use crate::{
    RustAnalysis, RustImport, RustPackageSources, RustPathRoot, RustReference, RustTargetSource,
    RustTypePath,
};

pub fn analyze_package(project_root: &Path, package: &RustPackageSources) -> RustAnalysis {
    Analyzer::new(project_root, package).run()
}

struct Analyzer<'a> {
    project_root: &'a Path,
    package: &'a RustPackageSources,
    source_files: BTreeSet<PathBuf>,
    parsed: BTreeMap<PathBuf, ParsedFile>,
    analyzed_files: BTreeSet<PathBuf>,
    output: RustAnalysis,
    type_contexts: Vec<TypeContext>,
    impls: Vec<ImplRecord>,
}

enum ParsedFile {
    Parsed(Rc<syn::File>),
    Failed,
}

#[derive(Clone, Debug)]
struct TypeContext {
    crate_key: String,
    module_id: SymbolId,
}

struct ImplRecord {
    scope: ReferenceScope,
    module_id: SymbolId,
    self_type_name: String,
    trait_path: Option<RustTypePath>,
    trait_display: Option<String>,
    impl_generics: Vec<String>,
    impl_references: Vec<PendingReference>,
    methods: Vec<MethodSpec>,
    location: SourceLocation,
}

#[derive(Clone, Debug)]
struct ReferenceScope {
    crate_key: String,
    module_path: Vec<String>,
    imports: Vec<RustImport>,
}

struct PendingReference {
    kind: RelationshipKind,
    path: RustTypePath,
    location: SourceLocation,
    note: Option<String>,
}

struct MethodSpec {
    name: String,
    parameters: Vec<Parameter>,
    return_type: Option<String>,
    generics: Vec<GenericParam>,
    visibility: Visibility,
    is_static: bool,
    location: SourceLocation,
    generic_parameters: Vec<String>,
    references: Vec<PendingReference>,
}

impl MethodSpec {
    fn into_method(self, id: SymbolId) -> Method {
        let method = Method::new(id, self.name, self.location)
            .with_parameters(self.parameters)
            .with_visibility(self.visibility)
            .with_generics(self.generics)
            .with_static(self.is_static);
        match self.return_type {
            Some(return_type) => method.with_return_type(return_type),
            None => method,
        }
    }
}

enum Resolution {
    Found(usize),
    Missing,
    Ambiguous(Vec<String>),
}

impl<'a> Analyzer<'a> {
    fn new(project_root: &'a Path, package: &'a RustPackageSources) -> Self {
        Self {
            project_root,
            package,
            source_files: BTreeSet::new(),
            parsed: BTreeMap::new(),
            analyzed_files: BTreeSet::new(),
            output: RustAnalysis::default(),
            type_contexts: Vec::new(),
            impls: Vec::new(),
        }
    }

    fn run(mut self) -> RustAnalysis {
        self.load_sources();
        self.analyze_targets();
        self.report_unreachable_sources();
        self.apply_impls();
        self.output
    }

    fn load_sources(&mut self) {
        let mut paths = BTreeSet::new();
        for path in self.package.source_files.iter().chain(
            self.package
                .targets
                .iter()
                .map(|target| &target.source_path),
        ) {
            if !is_safe_relative_path(path) {
                self.output.files_failed += 1;
                self.output.diagnostics.push(diagnostic(
                    DiagnosticSeverity::Error,
                    format!(
                        "source path is not project-relative and will not be read: {}",
                        path.display()
                    ),
                    None,
                    "rust::invalid-source-path",
                ));
                continue;
            }
            paths.insert(path.clone());
        }
        self.source_files = paths;

        for path in self.source_files.clone() {
            self.parse_source(&path);
        }
    }

    fn parse_source(&mut self, path: &Path) {
        let absolute = self.project_root.join(path);
        let source = match fs::read_to_string(&absolute) {
            Ok(source) => source,
            Err(error) => {
                self.output.files_failed += 1;
                self.output.diagnostics.push(diagnostic(
                    DiagnosticSeverity::Error,
                    format!("could not read Rust source `{}`: {error}", path.display()),
                    None,
                    "rust::unreadable-source",
                ));
                self.parsed.insert(path.to_path_buf(), ParsedFile::Failed);
                return;
            }
        };

        match syn::parse_file(&source) {
            Ok(file) => {
                self.output.files_analyzed += 1;
                self.parsed
                    .insert(path.to_path_buf(), ParsedFile::Parsed(Rc::new(file)));
            }
            Err(error) => {
                self.output.files_failed += 1;
                self.output.diagnostics.push(diagnostic(
                    DiagnosticSeverity::Error,
                    format!("failed to parse Rust source: {error}"),
                    Some(source_location(path, error.span())),
                    "rust::parse-error",
                ));
                self.parsed.insert(path.to_path_buf(), ParsedFile::Failed);
            }
        }
    }

    fn analyze_targets(&mut self) {
        let mut targets: Vec<&RustTargetSource> = self.package.targets.iter().collect();
        targets.sort_by(|left, right| {
            right
                .is_library
                .cmp(&left.is_library)
                .then_with(|| left.source_path.cmp(&right.source_path))
                .then_with(|| left.name.cmp(&right.name))
        });

        let mut crate_keys = BTreeSet::new();
        for target in targets {
            if !is_safe_relative_path(&target.source_path) {
                continue;
            }

            let crate_key = if target.is_library {
                self.package.name.clone()
            } else {
                format!("{}::{}", self.package.name, target.name)
            };
            if !crate_keys.insert(crate_key.clone()) {
                self.output.diagnostics.push(diagnostic(
                    DiagnosticSeverity::Warning,
                    format!(
                        "skipping duplicate Cargo target identity `{}` from `{}`",
                        crate_key,
                        target.source_path.display()
                    ),
                    None,
                    "rust::duplicate-target",
                ));
                continue;
            }

            let root_id = SymbolId::declaration(&target.source_path, &crate_key);
            let root_location = SourceLocation::new(&target.source_path, 1, 1, 1, 1);
            self.output.modules.push(Module::new(
                root_id.clone(),
                target.name.clone(),
                crate_key.clone(),
                target.source_path.clone(),
                root_location,
            ));

            let module_dir = target
                .source_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf();
            let mut visited = BTreeSet::new();
            visited.insert(target.source_path.clone());

            if let Some(parsed) = self.parsed_file(&target.source_path) {
                let mut module_path = Vec::new();
                self.analyze_items(
                    &parsed.items,
                    &crate_key,
                    &mut module_path,
                    &root_id,
                    &target.source_path,
                    &module_dir,
                    &mut visited,
                );
            }
            self.analyzed_files.extend(visited);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn analyze_items(
        &mut self,
        items: &[syn::Item],
        crate_key: &str,
        module_path: &mut Vec<String>,
        module_id: &SymbolId,
        file: &Path,
        module_dir: &Path,
        visited: &mut BTreeSet<PathBuf>,
    ) {
        let scope = ReferenceScope {
            crate_key: crate_key.to_owned(),
            module_path: module_path.clone(),
            imports: imports_from_items(items),
        };
        for item in items {
            match item {
                syn::Item::Mod(item_mod) => self.lower_mod(
                    item_mod,
                    crate_key,
                    module_path,
                    module_id,
                    file,
                    module_dir,
                    visited,
                ),
                syn::Item::Struct(item_struct) => {
                    self.lower_struct(item_struct, &scope, module_id, file);
                }
                syn::Item::Enum(item_enum) => {
                    self.lower_enum(item_enum, &scope, module_id, file);
                }
                syn::Item::Union(item_union) => {
                    self.lower_union(item_union, &scope, module_id, file);
                }
                syn::Item::Type(item_type) => {
                    self.lower_type_alias(item_type, &scope, module_id, file);
                }
                syn::Item::Trait(item_trait) => {
                    self.lower_trait(item_trait, &scope, module_id, file);
                }
                syn::Item::Impl(item_impl) => {
                    self.lower_impl(item_impl, &scope, module_id, file);
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_mod(
        &mut self,
        item_mod: &syn::ItemMod,
        crate_key: &str,
        module_path: &mut Vec<String>,
        module_id: &SymbolId,
        file: &Path,
        module_dir: &Path,
        visited: &mut BTreeSet<PathBuf>,
    ) {
        let name = item_mod.ident.to_string();
        let location = source_location(file, item_mod.span());
        module_path.push(name.clone());
        let qualified_name = qualify(crate_key, module_path);

        match &item_mod.content {
            Some((_, items)) => {
                let child_id = SymbolId::declaration(file, &qualified_name);
                let child_dir = push_segment(module_dir, &name);
                self.output.modules.push(
                    Module::new(child_id.clone(), name, qualified_name, file, location)
                        .with_parent(module_id.clone()),
                );
                self.analyze_items(
                    items,
                    crate_key,
                    module_path,
                    &child_id,
                    file,
                    &child_dir,
                    visited,
                );
            }
            None => {
                if item_mod
                    .attrs
                    .iter()
                    .any(|attribute| attribute.path().is_ident("path"))
                {
                    self.output.diagnostics.push(diagnostic(
                        DiagnosticSeverity::Warning,
                        "modules with `#[path = \"...\"]` are not resolved; the module was skipped",
                        Some(location),
                        "rust::unsupported-path-attribute",
                    ));
                    module_path.pop();
                    return;
                }

                let file_candidate = push_segment(module_dir, &format!("{name}.rs"));
                let directory_candidate = push_segment(module_dir, &name);
                let mod_rs_candidate = push_segment(&directory_candidate, "mod.rs");
                let candidates: Vec<PathBuf> = [file_candidate, mod_rs_candidate]
                    .into_iter()
                    .filter(|candidate| self.source_files.contains(candidate))
                    .collect();

                let Some(module_file) = candidates.first().cloned() else {
                    self.output.diagnostics.push(diagnostic(
                        DiagnosticSeverity::Warning,
                        format!(
                            "external module `{name}` declared in `{}` has no matching source file",
                            file.display()
                        ),
                        Some(location.clone()),
                        "rust::unresolved-module",
                    ));
                    module_path.pop();
                    return;
                };
                if candidates.len() > 1 {
                    self.output.diagnostics.push(diagnostic(
                        DiagnosticSeverity::Warning,
                        format!(
                            "multiple source files match external module `{name}`; using `{}`",
                            module_file.display()
                        ),
                        Some(location.clone()),
                        "rust::ambiguous-module",
                    ));
                }

                let child_id = SymbolId::declaration(&module_file, &qualified_name);
                self.output.modules.push(
                    Module::new(
                        child_id.clone(),
                        name.clone(),
                        qualified_name,
                        &module_file,
                        location.clone(),
                    )
                    .with_parent(module_id.clone()),
                );

                if !visited.insert(module_file.clone()) {
                    self.output.diagnostics.push(diagnostic(
                        DiagnosticSeverity::Warning,
                        format!(
                            "source file `{}` is referenced by more than one module path in this crate",
                            module_file.display()
                        ),
                        Some(location.clone()),
                        "rust::duplicate-module-file",
                    ));
                } else if let Some(parsed) = self.parsed_file(&module_file) {
                    let child_dir = push_segment(module_dir, &name);
                    self.analyze_items(
                        &parsed.items,
                        crate_key,
                        module_path,
                        &child_id,
                        &module_file,
                        &child_dir,
                        visited,
                    );
                }
            }
        }

        module_path.pop();
    }

    fn lower_struct(
        &mut self,
        item: &syn::ItemStruct,
        scope: &ReferenceScope,
        module_id: &SymbolId,
        file: &Path,
    ) {
        let name = item.ident.to_string();
        let qualified_name = type_qualified_name(&scope.crate_key, &scope.module_path, &name);
        let type_id = SymbolId::declaration(file, &qualified_name);
        let generic_names = generic_names(&item.generics);
        let mut declaration = TypeDecl::new(
            type_id.clone(),
            name,
            qualified_name,
            TypeKind::Struct,
            source_location(file, item.span()),
        )
        .with_module(module_id.clone())
        .with_visibility(visibility(&item.vis))
        .with_generics(generic_params(&item.generics));
        declaration.fields = lower_fields(&type_id, &item.fields, file);

        let mut references = field_references(
            &item.fields,
            &declaration.fields,
            RelationshipKind::FieldType,
            scope,
            &generic_names,
            file,
        );
        emit_pending(
            scope,
            &type_id,
            &generic_names,
            &generic_bound_references(&item.generics, file),
            &mut references,
        );
        self.push_type(declaration, scope, module_id);
        self.output.references.extend(references);
    }

    fn lower_enum(
        &mut self,
        item: &syn::ItemEnum,
        scope: &ReferenceScope,
        module_id: &SymbolId,
        file: &Path,
    ) {
        let name = item.ident.to_string();
        let qualified_name = type_qualified_name(&scope.crate_key, &scope.module_path, &name);
        let type_id = SymbolId::declaration(file, &qualified_name);
        let generic_names = generic_names(&item.generics);
        let mut declaration = TypeDecl::new(
            type_id.clone(),
            name,
            qualified_name,
            TypeKind::Enum,
            source_location(file, item.span()),
        )
        .with_module(module_id.clone())
        .with_visibility(visibility(&item.vis))
        .with_generics(generic_params(&item.generics));

        let mut references = Vec::new();
        for variant in &item.variants {
            let variant_name = variant.ident.to_string();
            let variant_id = SymbolId::variant(&type_id, &variant_name);
            let mut lowered = Variant::new(
                variant_id.clone(),
                variant_name,
                source_location(file, variant.span()),
            );
            lowered.fields = lower_fields(&variant_id, &variant.fields, file);
            references.extend(field_references(
                &variant.fields,
                &lowered.fields,
                RelationshipKind::FieldType,
                scope,
                &generic_names,
                file,
            ));
            declaration.variants.push(lowered);
        }
        emit_pending(
            scope,
            &type_id,
            &generic_names,
            &generic_bound_references(&item.generics, file),
            &mut references,
        );
        self.push_type(declaration, scope, module_id);
        self.output.references.extend(references);
    }

    fn lower_union(
        &mut self,
        item: &syn::ItemUnion,
        scope: &ReferenceScope,
        module_id: &SymbolId,
        file: &Path,
    ) {
        let name = item.ident.to_string();
        let qualified_name = type_qualified_name(&scope.crate_key, &scope.module_path, &name);
        let type_id = SymbolId::declaration(file, &qualified_name);
        let generic_names = generic_names(&item.generics);
        let mut declaration = TypeDecl::new(
            type_id.clone(),
            name,
            qualified_name,
            TypeKind::Union,
            source_location(file, item.span()),
        )
        .with_module(module_id.clone())
        .with_visibility(visibility(&item.vis))
        .with_generics(generic_params(&item.generics));
        let fields = syn::Fields::Named(item.fields.clone());
        declaration.fields = lower_fields(&type_id, &fields, file);

        let mut references = field_references(
            &fields,
            &declaration.fields,
            RelationshipKind::FieldType,
            scope,
            &generic_names,
            file,
        );
        emit_pending(
            scope,
            &type_id,
            &generic_names,
            &generic_bound_references(&item.generics, file),
            &mut references,
        );
        self.push_type(declaration, scope, module_id);
        self.output.references.extend(references);
    }

    fn lower_type_alias(
        &mut self,
        item: &syn::ItemType,
        scope: &ReferenceScope,
        module_id: &SymbolId,
        file: &Path,
    ) {
        let name = item.ident.to_string();
        let qualified_name = type_qualified_name(&scope.crate_key, &scope.module_path, &name);
        let type_id = SymbolId::declaration(file, &qualified_name);
        let generic_names = generic_names(&item.generics);
        let declaration = TypeDecl::new(
            type_id.clone(),
            name,
            qualified_name,
            TypeKind::TypeAlias,
            source_location(file, item.span()),
        )
        .with_alias_of(tokens_string(&item.ty))
        .with_module(module_id.clone())
        .with_visibility(visibility(&item.vis))
        .with_generics(generic_params(&item.generics));

        let mut pending = Vec::new();
        extract_type_references(
            &item.ty,
            RelationshipKind::Aliases,
            file,
            Some("alias target".to_owned()),
            &mut pending,
        );
        let mut references = Vec::new();
        emit_pending(scope, &type_id, &generic_names, &pending, &mut references);
        emit_pending(
            scope,
            &type_id,
            &generic_names,
            &generic_bound_references(&item.generics, file),
            &mut references,
        );
        self.push_type(declaration, scope, module_id);
        self.output.references.extend(references);
    }

    fn lower_trait(
        &mut self,
        item: &syn::ItemTrait,
        scope: &ReferenceScope,
        module_id: &SymbolId,
        file: &Path,
    ) {
        let name = item.ident.to_string();
        let qualified_name = type_qualified_name(&scope.crate_key, &scope.module_path, &name);
        let type_id = SymbolId::declaration(file, &qualified_name);
        let generic_names = generic_names(&item.generics);
        let mut declaration = TypeDecl::new(
            type_id.clone(),
            name,
            qualified_name,
            TypeKind::Trait,
            source_location(file, item.span()),
        )
        .with_module(module_id.clone())
        .with_visibility(visibility(&item.vis))
        .with_generics(generic_params(&item.generics));

        let mut references = Vec::new();
        let mut supertraits = Vec::new();
        for bound in &item.supertraits {
            extract_bound_reference(bound, file, "supertrait".to_owned(), &mut supertraits);
        }
        emit_pending(
            scope,
            &type_id,
            &generic_names,
            &supertraits,
            &mut references,
        );
        emit_pending(
            scope,
            &type_id,
            &generic_names,
            &generic_bound_references(&item.generics, file),
            &mut references,
        );

        for trait_item in &item.items {
            if let syn::TraitItem::Fn(trait_method) = trait_item {
                let spec = method_spec(
                    &trait_method.sig,
                    source_location(file, trait_method.span()),
                    Visibility::Public,
                    file,
                );
                let method_id = SymbolId::method(&type_id, &spec.name);
                let mut method_generics = generic_names.clone();
                method_generics.extend(spec.generic_parameters.iter().cloned());
                method_generics.sort();
                method_generics.dedup();
                emit_pending(
                    scope,
                    &method_id,
                    &method_generics,
                    &spec.references,
                    &mut references,
                );
                declaration.methods.push(spec.into_method(method_id));
            }
        }
        self.push_type(declaration, scope, module_id);
        self.output.references.extend(references);
    }

    fn lower_impl(
        &mut self,
        item: &syn::ItemImpl,
        scope: &ReferenceScope,
        module_id: &SymbolId,
        file: &Path,
    ) {
        let Some(self_type_name) = self_type_name(&item.self_ty) else {
            self.output.diagnostics.push(diagnostic(
                DiagnosticSeverity::Warning,
                format!(
                    "could not identify the type targeted by an impl block in `{}`",
                    file.display()
                ),
                Some(source_location(file, item.span())),
                "rust::unsupported-impl-target",
            ));
            return;
        };

        let (trait_path, trait_display) = match &item.trait_ {
            Some((_, path, _)) => {
                let display = tokens_string(path);
                (Some(type_path_from_syn_path(path)), Some(display))
            }
            None => (None, None),
        };

        let trait_impl = item.trait_.is_some();
        let mut methods = Vec::new();
        for impl_item in &item.items {
            if let syn::ImplItem::Fn(impl_method) = impl_item {
                methods.push(method_spec(
                    &impl_method.sig,
                    source_location(file, impl_method.span()),
                    if trait_impl {
                        Visibility::Public
                    } else {
                        visibility(&impl_method.vis)
                    },
                    file,
                ));
            }
        }

        self.impls.push(ImplRecord {
            scope: scope.clone(),
            module_id: module_id.clone(),
            self_type_name,
            trait_path,
            trait_display,
            impl_generics: generic_names(&item.generics),
            impl_references: generic_bound_references(&item.generics, file),
            methods,
            location: source_location(file, item.span()),
        });
    }

    fn push_type(&mut self, declaration: TypeDecl, scope: &ReferenceScope, module_id: &SymbolId) {
        self.type_contexts.push(TypeContext {
            crate_key: scope.crate_key.clone(),
            module_id: module_id.clone(),
        });
        self.output.types.push(declaration);
    }

    fn apply_impls(&mut self) {
        for record in std::mem::take(&mut self.impls) {
            self.apply_impl(record);
        }
    }

    fn apply_impl(&mut self, record: ImplRecord) {
        let scope = record.scope.clone();
        let location = record.location.clone();
        let source_resolution = resolve_named_type(
            &self.output.types,
            &self.type_contexts,
            &scope.crate_key,
            &record.module_id,
            &record.self_type_name,
            None,
        );
        let source_index = match source_resolution {
            Resolution::Found(index) => index,
            Resolution::Missing => {
                self.output.diagnostics.push(diagnostic(
                    DiagnosticSeverity::Warning,
                    format!(
                        "impl target `{}` is not declared in the analyzed crate; methods were not attached",
                        record.self_type_name
                    ),
                    Some(location),
                    "rust::unresolved-impl-target",
                ));
                return;
            }
            Resolution::Ambiguous(names) => {
                self.output.diagnostics.push(diagnostic(
                    DiagnosticSeverity::Warning,
                    format!(
                        "impl target `{}` is ambiguous ({}); methods were not attached",
                        record.self_type_name,
                        names.join(", ")
                    ),
                    Some(location),
                    "rust::ambiguous-impl-target",
                ));
                return;
            }
        };

        let type_id = self.output.types[source_index].id.clone();
        let mut all_generics = record.impl_generics.clone();
        all_generics.extend(generic_names_from_params(
            &self.output.types[source_index].generics,
        ));
        all_generics.sort();
        all_generics.dedup();

        let mut references = Vec::new();
        emit_pending(
            &scope,
            &type_id,
            &all_generics,
            &record.impl_references,
            &mut references,
        );
        if let Some(trait_path) = &record.trait_path {
            let trait_reference = PendingReference {
                kind: RelationshipKind::Implements,
                path: trait_path.clone(),
                location: location.clone(),
                note: Some("trait implementation".to_owned()),
            };
            emit_pending(
                &scope,
                &type_id,
                &all_generics,
                &[trait_reference],
                &mut references,
            );
        }

        let mut duplicate_methods = Vec::new();
        {
            let declaration = &mut self.output.types[source_index];
            let mut ids: BTreeSet<SymbolId> = declaration
                .methods
                .iter()
                .map(|method| method.id.clone())
                .collect();

            for mut spec in record.methods {
                let method_id = match &record.trait_display {
                    Some(trait_display) => SymbolId::method_in(&type_id, trait_display, &spec.name),
                    None => SymbolId::method(&type_id, &spec.name),
                };
                let mut method_generics = all_generics.clone();
                method_generics.extend(spec.generic_parameters.iter().cloned());
                method_generics.sort();
                method_generics.dedup();
                if ids.insert(method_id.clone()) {
                    emit_pending(
                        &scope,
                        &method_id,
                        &method_generics,
                        &spec.references,
                        &mut references,
                    );
                    spec.references.clear();
                    declaration.methods.push(spec.into_method(method_id));
                } else {
                    duplicate_methods.push((
                        spec.name,
                        spec.location,
                        format!(
                            "duplicate method `{}` for `{}` was skipped",
                            declaration.name, record.self_type_name
                        ),
                    ));
                }
            }
        }
        for (name, method_location, message) in duplicate_methods {
            self.output.diagnostics.push(diagnostic(
                DiagnosticSeverity::Warning,
                format!("{message} (method `{name}`)"),
                Some(method_location),
                "rust::duplicate-method",
            ));
        }

        self.output.references.extend(references);
    }

    fn report_unreachable_sources(&mut self) {
        for path in &self.source_files {
            if !self.analyzed_files.contains(path) {
                self.output.diagnostics.push(diagnostic(
                    DiagnosticSeverity::Warning,
                    format!(
                        "source file `{}` is not reachable from a Cargo target",
                        path.display()
                    ),
                    None,
                    "rust::unreachable-source",
                ));
            }
        }
    }

    fn parsed_file(&self, path: &Path) -> Option<Rc<syn::File>> {
        match self.parsed.get(path) {
            Some(ParsedFile::Parsed(file)) => Some(file.clone()),
            Some(ParsedFile::Failed) | None => None,
        }
    }
}

fn resolve_named_type(
    types: &[TypeDecl],
    contexts: &[TypeContext],
    crate_key: &str,
    module_id: &SymbolId,
    name: &str,
    kind: Option<&TypeKind>,
) -> Resolution {
    let mut candidates: Vec<usize> = types
        .iter()
        .enumerate()
        .filter(|(index, declaration)| {
            contexts[*index].crate_key == crate_key
                && declaration.name == name
                && kind.is_none_or(|kind| &declaration.kind == kind)
        })
        .map(|(index, _)| index)
        .collect();

    let same_module: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|index| &contexts[*index].module_id == module_id)
        .collect();
    if !same_module.is_empty() {
        candidates = same_module;
    }

    match candidates.as_slice() {
        [] => Resolution::Missing,
        [index] => Resolution::Found(*index),
        _ => Resolution::Ambiguous(
            candidates
                .iter()
                .map(|index| types[*index].qualified_name.clone())
                .collect(),
        ),
    }
}

fn lower_fields(parent_id: &SymbolId, fields: &syn::Fields, file: &Path) -> Vec<Field> {
    match fields {
        syn::Fields::Named(named) => named
            .named
            .iter()
            .map(|field| {
                let name = field
                    .ident
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "_".to_owned());
                lower_field(parent_id, name, field, file)
            })
            .collect(),
        syn::Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .enumerate()
            .map(|(index, field)| lower_field(parent_id, index.to_string(), field, file))
            .collect(),
        syn::Fields::Unit => Vec::new(),
    }
}

fn lower_field(parent_id: &SymbolId, name: String, field: &syn::Field, file: &Path) -> Field {
    let field_id = SymbolId::field(parent_id, &name);
    Field::new(field_id, name, source_location(file, field.span()))
        .with_type(tokens_string(&field.ty))
        .with_visibility(visibility(&field.vis))
}

fn method_spec(
    signature: &syn::Signature,
    location: SourceLocation,
    visibility: Visibility,
    file: &Path,
) -> MethodSpec {
    let mut references = Vec::new();
    let parameters = signature
        .inputs
        .iter()
        .filter_map(|argument| match argument {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(pat_type) => {
                let name = pattern_name(&pat_type.pat);
                extract_type_references(
                    &pat_type.ty,
                    RelationshipKind::MethodParameterType,
                    file,
                    Some(format!("parameter `{name}`")),
                    &mut references,
                );
                Some(Parameter::new(name, tokens_string(&pat_type.ty)))
            }
        })
        .collect();
    let return_type = match &signature.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, return_type) => {
            extract_type_references(
                return_type,
                RelationshipKind::MethodReturnType,
                file,
                Some("return type".to_owned()),
                &mut references,
            );
            Some(tokens_string(return_type))
        }
    };
    references.extend(generic_bound_references(&signature.generics, file));

    MethodSpec {
        name: signature.ident.to_string(),
        parameters,
        return_type,
        generics: generic_params(&signature.generics),
        visibility,
        is_static: signature.receiver().is_none(),
        location,
        generic_parameters: generic_names(&signature.generics),
        references,
    }
}

fn field_references(
    syn_fields: &syn::Fields,
    lowered_fields: &[Field],
    kind: RelationshipKind,
    scope: &ReferenceScope,
    generic_parameters: &[String],
    file: &Path,
) -> Vec<RustReference> {
    let mut references = Vec::new();
    let pairs: Vec<(&syn::Field, &Field)> = match syn_fields {
        syn::Fields::Named(named) => named.named.iter().zip(lowered_fields).collect(),
        syn::Fields::Unnamed(unnamed) => unnamed.unnamed.iter().zip(lowered_fields).collect(),
        syn::Fields::Unit => Vec::new(),
    };
    for (syn_field, lowered) in pairs {
        let mut pending = Vec::new();
        extract_type_references(
            &syn_field.ty,
            kind,
            file,
            Some(format!("field `{}`", lowered.name)),
            &mut pending,
        );
        emit_pending(
            scope,
            &lowered.id,
            generic_parameters,
            &pending,
            &mut references,
        );
    }
    references
}

fn emit_pending(
    scope: &ReferenceScope,
    source: &SymbolId,
    generic_parameters: &[String],
    pending: &[PendingReference],
    out: &mut Vec<RustReference>,
) {
    for reference in pending {
        out.push(RustReference {
            source: source.clone(),
            kind: reference.kind,
            path: reference.path.clone(),
            location: reference.location.clone(),
            crate_key: scope.crate_key.clone(),
            module_path: scope.module_path.clone(),
            generic_parameters: generic_parameters.to_vec(),
            imports: scope.imports.clone(),
            note: reference.note.clone(),
        });
    }
}

fn generic_bound_references(generics: &syn::Generics, file: &Path) -> Vec<PendingReference> {
    let mut references = Vec::new();
    for parameter in &generics.params {
        if let syn::GenericParam::Type(parameter) = parameter {
            for bound in &parameter.bounds {
                extract_bound_reference(
                    bound,
                    file,
                    format!("generic bound `{}`", parameter.ident),
                    &mut references,
                );
            }
        }
    }
    if let Some(where_clause) = &generics.where_clause {
        for predicate in &where_clause.predicates {
            if let syn::WherePredicate::Type(predicate) = predicate {
                for bound in &predicate.bounds {
                    extract_bound_reference(
                        bound,
                        file,
                        "where-clause bound".to_owned(),
                        &mut references,
                    );
                }
            }
        }
    }
    references
}

fn extract_bound_reference(
    bound: &syn::TypeParamBound,
    file: &Path,
    note: String,
    out: &mut Vec<PendingReference>,
) {
    if let syn::TypeParamBound::Trait(bound) = bound {
        let path = type_path_from_syn_path(&bound.path);
        out.push(PendingReference {
            kind: RelationshipKind::TraitBound,
            path,
            location: source_location(file, bound.path.span()),
            note: Some(note),
        });
        extract_path_arguments(&bound.path, RelationshipKind::References, file, None, out);
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_type_references(
    ty: &syn::Type,
    kind: RelationshipKind,
    file: &Path,
    note: Option<String>,
    out: &mut Vec<PendingReference>,
) {
    match ty {
        syn::Type::Path(type_path) => {
            if let Some(qself) = &type_path.qself {
                extract_type_references(&qself.ty, kind, file, note.clone(), out);
                let trait_path = path_subset(
                    &type_path.path,
                    qself.position,
                    type_path.path.leading_colon.is_some(),
                );
                if !trait_path.segments.is_empty() {
                    out.push(PendingReference {
                        kind: RelationshipKind::TraitBound,
                        path: trait_path,
                        location: source_location(file, type_path.span()),
                        note: Some("qualified associated-type bound".to_owned()),
                    });
                }
                out.push(PendingReference {
                    kind,
                    path: RustTypePath {
                        root: RustPathRoot::Unresolved,
                        segments: Vec::new(),
                        display: tokens_string(ty),
                    },
                    location: source_location(file, type_path.span()),
                    note: note.clone(),
                });
            } else {
                let path = type_path_from_syn_path(&type_path.path);
                if !path.segments.is_empty() {
                    out.push(PendingReference {
                        kind,
                        path,
                        location: source_location(file, type_path.path.span()),
                        note: note.clone(),
                    });
                }
            }
            extract_path_arguments(&type_path.path, kind, file, note, out);
        }
        syn::Type::Reference(reference) => {
            extract_type_references(&reference.elem, kind, file, note, out);
        }
        syn::Type::Ptr(pointer) => {
            extract_type_references(&pointer.elem, kind, file, note, out);
        }
        syn::Type::Slice(slice) => {
            extract_type_references(&slice.elem, kind, file, note, out);
        }
        syn::Type::Array(array) => {
            extract_type_references(&array.elem, kind, file, note, out);
        }
        syn::Type::Tuple(tuple) => {
            for element in &tuple.elems {
                extract_type_references(element, kind, file, note.clone(), out);
            }
        }
        syn::Type::Group(group) => {
            extract_type_references(&group.elem, kind, file, note, out);
        }
        syn::Type::Paren(paren) => {
            extract_type_references(&paren.elem, kind, file, note, out);
        }
        syn::Type::BareFn(function) => {
            for input in &function.inputs {
                extract_type_references(&input.ty, kind, file, note.clone(), out);
            }
            if let syn::ReturnType::Type(_, return_type) = &function.output {
                extract_type_references(return_type, kind, file, note.clone(), out);
            }
        }
        syn::Type::TraitObject(object) => {
            for bound in &object.bounds {
                extract_bound_reference(bound, file, "trait object bound".to_owned(), out);
            }
        }
        syn::Type::ImplTrait(implementation) => {
            for bound in &implementation.bounds {
                extract_bound_reference(bound, file, "impl-trait bound".to_owned(), out);
            }
        }
        _ => {}
    }
}

fn extract_path_arguments(
    path: &syn::Path,
    kind: RelationshipKind,
    file: &Path,
    note: Option<String>,
    out: &mut Vec<PendingReference>,
) {
    for segment in &path.segments {
        match &segment.arguments {
            syn::PathArguments::None => {}
            syn::PathArguments::AngleBracketed(arguments) => {
                extract_angle_arguments(arguments, kind, file, note.clone(), out);
            }
            syn::PathArguments::Parenthesized(arguments) => {
                for input in &arguments.inputs {
                    extract_type_references(input, kind, file, note.clone(), out);
                }
                if let syn::ReturnType::Type(_, return_type) = &arguments.output {
                    extract_type_references(return_type, kind, file, note.clone(), out);
                }
            }
        }
    }
}

fn extract_angle_arguments(
    arguments: &syn::AngleBracketedGenericArguments,
    kind: RelationshipKind,
    file: &Path,
    note: Option<String>,
    out: &mut Vec<PendingReference>,
) {
    for argument in &arguments.args {
        match argument {
            syn::GenericArgument::Type(ty) => {
                extract_type_references(ty, kind, file, note.clone(), out);
            }
            syn::GenericArgument::AssocType(assoc) => {
                extract_type_references(&assoc.ty, kind, file, note.clone(), out);
                if let Some(generics) = &assoc.generics {
                    extract_angle_arguments(generics, kind, file, note.clone(), out);
                }
            }
            syn::GenericArgument::Constraint(constraint) => {
                for bound in &constraint.bounds {
                    extract_bound_reference(bound, file, "associated-type bound".to_owned(), out);
                }
            }
            _ => {}
        }
    }
}

fn type_path_from_syn_path(path: &syn::Path) -> RustTypePath {
    rust_type_path(
        path.leading_colon.is_some(),
        path.segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect(),
    )
}

fn path_subset(path: &syn::Path, end: usize, leading_colon: bool) -> RustTypePath {
    rust_type_path(
        leading_colon,
        path.segments
            .iter()
            .take(end)
            .map(|segment| segment.ident.to_string())
            .collect(),
    )
}

fn rust_type_path(leading_global: bool, mut segments: Vec<String>) -> RustTypePath {
    let mut root = if leading_global {
        RustPathRoot::Global
    } else {
        RustPathRoot::Relative
    };
    if !leading_global {
        if segments.first().is_some_and(|segment| segment == "crate") {
            root = RustPathRoot::Crate;
            segments.remove(0);
        } else if segments.first().is_some_and(|segment| segment == "self") {
            root = RustPathRoot::SelfModule;
            segments.remove(0);
        } else {
            let mut super_count = 0;
            while segments.first().is_some_and(|segment| segment == "super") {
                segments.remove(0);
                super_count += 1;
            }
            if super_count > 0 {
                root = RustPathRoot::Super(super_count);
            }
        }
    }

    let mut display = match &root {
        RustPathRoot::Crate => "crate::".to_owned(),
        RustPathRoot::SelfModule => "self::".to_owned(),
        RustPathRoot::Super(count) => "super::".repeat(*count),
        RustPathRoot::Global => "::".to_owned(),
        RustPathRoot::Relative | RustPathRoot::Unresolved => String::new(),
    };
    display.push_str(&segments.join("::"));

    RustTypePath {
        root,
        segments,
        display,
    }
}

fn imports_from_items(items: &[syn::Item]) -> Vec<RustImport> {
    let mut imports = Vec::new();
    for item in items {
        if let syn::Item::Use(item_use) = item {
            flatten_use_tree(
                &item_use.tree,
                &mut Vec::new(),
                item_use.leading_colon.is_some(),
                &mut imports,
            );
        }
    }
    imports
}

fn flatten_use_tree(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    leading_global: bool,
    out: &mut Vec<RustImport>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            flatten_use_tree(&path.tree, prefix, leading_global, out);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            if name.ident == "self" {
                if let Some(alias) = prefix.last() {
                    out.push(RustImport::Named {
                        alias: alias.clone(),
                        path: rust_type_path(leading_global, prefix.clone()),
                    });
                }
            } else {
                let alias = name.ident.to_string();
                let mut segments = prefix.clone();
                segments.push(alias.clone());
                out.push(RustImport::Named {
                    alias,
                    path: rust_type_path(leading_global, segments),
                });
            }
        }
        syn::UseTree::Rename(rename) => {
            let mut segments = prefix.clone();
            if rename.ident != "self" {
                segments.push(rename.ident.to_string());
            }
            if !segments.is_empty() {
                out.push(RustImport::Named {
                    alias: rename.rename.to_string(),
                    path: rust_type_path(leading_global, segments),
                });
            }
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                flatten_use_tree(item, prefix, leading_global, out);
            }
        }
        syn::UseTree::Glob(_) => {
            if !prefix.is_empty() {
                out.push(RustImport::Glob {
                    path: rust_type_path(leading_global, prefix.clone()),
                });
            }
        }
    }
}

fn generic_names(generics: &syn::Generics) -> Vec<String> {
    let mut names: Vec<String> = generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            syn::GenericParam::Type(parameter) => Some(parameter.ident.to_string()),
            syn::GenericParam::Const(parameter) => Some(parameter.ident.to_string()),
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

fn generic_names_from_params(generics: &[GenericParam]) -> Vec<String> {
    let mut names: Vec<String> = generics
        .iter()
        .filter(|generic| !generic.name.starts_with('\''))
        .map(|generic| generic.name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn pattern_name(pattern: &syn::Pat) -> String {
    match pattern {
        syn::Pat::Ident(pattern) => pattern.ident.to_string(),
        _ => "_".to_owned(),
    }
}

fn generic_params(generics: &syn::Generics) -> Vec<GenericParam> {
    generics
        .params
        .iter()
        .map(|generic| match generic {
            syn::GenericParam::Type(parameter) => {
                let name = parameter.ident.to_string();
                let mut bounds: Vec<String> = parameter.bounds.iter().map(tokens_string).collect();
                bounds.extend(where_type_bounds(generics, &name));
                let mut lowered = GenericParam::new(&name).with_bounds(bounds);
                if let Some(default) = &parameter.default {
                    lowered = lowered.with_default(tokens_string(default));
                }
                lowered
            }
            syn::GenericParam::Lifetime(parameter) => {
                let name = format!("'{}", parameter.lifetime.ident);
                let mut bounds: Vec<String> = parameter.bounds.iter().map(tokens_string).collect();
                bounds.extend(where_lifetime_bounds(generics, &parameter.lifetime));
                GenericParam::new(name).with_bounds(bounds)
            }
            syn::GenericParam::Const(parameter) => {
                let name = parameter.ident.to_string();
                let mut lowered = GenericParam::new(name);
                if let Some(default) = &parameter.default {
                    lowered = lowered.with_default(tokens_string(default));
                }
                lowered
            }
        })
        .collect()
}

fn where_type_bounds(generics: &syn::Generics, name: &str) -> Vec<String> {
    let Some(where_clause) = &generics.where_clause else {
        return Vec::new();
    };
    let mut bounds = Vec::new();
    for predicate in &where_clause.predicates {
        if let syn::WherePredicate::Type(predicate) = predicate {
            if type_is_ident(&predicate.bounded_ty, name) {
                bounds.extend(predicate.bounds.iter().map(tokens_string));
            }
        }
    }
    bounds
}

fn where_lifetime_bounds(generics: &syn::Generics, lifetime: &syn::Lifetime) -> Vec<String> {
    let Some(where_clause) = &generics.where_clause else {
        return Vec::new();
    };
    let mut bounds = Vec::new();
    for predicate in &where_clause.predicates {
        if let syn::WherePredicate::Lifetime(predicate) = predicate {
            if predicate.lifetime.ident == lifetime.ident {
                bounds.extend(predicate.bounds.iter().map(tokens_string));
            }
        }
    }
    bounds
}

fn type_is_ident(ty: &syn::Type, name: &str) -> bool {
    match ty {
        syn::Type::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => {
            path.path.segments[0].ident == name
        }
        _ => false,
    }
}

fn visibility(visibility: &syn::Visibility) -> Visibility {
    match visibility {
        syn::Visibility::Public(_) => Visibility::Public,
        syn::Visibility::Restricted(restricted) => {
            if restricted.in_token.is_none() {
                if restricted.path.is_ident("crate") {
                    return Visibility::Crate;
                }
                if restricted.path.is_ident("super") {
                    return Visibility::Super;
                }
                if restricted.path.is_ident("self") {
                    return Visibility::Private;
                }
            }
            Visibility::Restricted(tokens_string(&restricted.path))
        }
        syn::Visibility::Inherited => Visibility::Private,
    }
}

fn self_type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        syn::Type::Group(group) => self_type_name(&group.elem),
        syn::Type::Paren(paren) => self_type_name(&paren.elem),
        _ => None,
    }
}

fn type_qualified_name(crate_key: &str, module_path: &[String], name: &str) -> String {
    let mut qualified = qualify(crate_key, module_path);
    qualified.push_str("::");
    qualified.push_str(name);
    qualified
}

fn qualify(crate_key: &str, module_path: &[String]) -> String {
    let mut qualified = crate_key.to_owned();
    for segment in module_path {
        qualified.push_str("::");
        qualified.push_str(segment);
    }
    qualified
}

fn push_segment(base: &Path, segment: &str) -> PathBuf {
    let base = base.to_string_lossy().replace('\\', "/");
    let base = base.trim_end_matches('/');
    if base.is_empty() || base == "." {
        PathBuf::from(segment)
    } else {
        PathBuf::from(format!("{base}/{segment}"))
    }
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn line_col(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn source_location(file: &Path, span: Span) -> SourceLocation {
    let start = span.start();
    let end = span.end();
    SourceLocation::new(
        file,
        line_col(start.line),
        line_col(start.column.saturating_add(1)),
        line_col(end.line),
        line_col(end.column.saturating_add(1)),
    )
}

fn tokens_string<T: ToTokens + ?Sized>(tokens: &T) -> String {
    normalize_tokens(&tokens.to_token_stream().to_string())
}

/// `proc_macro2`'s token display inserts spaces around punctuation. Normalize
/// only the punctuation relevant to Rust type and visibility rendering.
fn normalize_tokens(text: &str) -> String {
    let mut normalized = text.to_owned();
    for (from, to) in [
        (" :: ", "::"),
        (":: ", "::"),
        (" ::", "::"),
        (" < ", "<"),
        ("< ", "<"),
        (" <", "<"),
        (" >", ">"),
        (" ,", ","),
        ("& ", "&"),
        ("' ", "'"),
        ("* ", "*"),
    ] {
        while normalized.contains(from) {
            normalized = normalized.replace(from, to);
        }
    }
    normalized
}

fn diagnostic(
    severity: DiagnosticSeverity,
    message: impl Into<String>,
    location: Option<SourceLocation>,
    code: &'static str,
) -> Diagnostic {
    Diagnostic::new(severity, message, location).with_code(code)
}
