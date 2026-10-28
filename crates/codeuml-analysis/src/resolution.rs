use std::collections::{BTreeMap, BTreeSet};

use codeuml_core::{
    Codebase, Diagnostic, DiagnosticSeverity, Provenance, Relationship, RelationshipEvidence,
    RelationshipTarget, ResolutionStatus, SymbolId, UnresolvedReference,
};
use codeuml_lang_rust::{RustImport, RustPathRoot, RustReference, RustTypePath};

const PRIMITIVE_TYPES: &[&str] = &[
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f16", "f32", "f64", "f128",
];

const KNOWN_EXTERNAL_TYPES: &[&str] = &[
    "Arc",
    "BTreeMap",
    "BTreeSet",
    "Box",
    "Cell",
    "Clone",
    "Copy",
    "Cow",
    "Debug",
    "Default",
    "Display",
    "Drop",
    "Eq",
    "Error",
    "Fn",
    "FnMut",
    "FnOnce",
    "From",
    "Hash",
    "HashMap",
    "HashSet",
    "Into",
    "Iterator",
    "LinkedList",
    "Option",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "PhantomData",
    "Rc",
    "RefCell",
    "Result",
    "Send",
    "Sized",
    "String",
    "Sync",
    "TryFrom",
    "TryInto",
    "Unpin",
    "Vec",
    "VecDeque",
    "bool",
    "char",
    "f32",
    "f64",
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "str",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
];

const KNOWN_EXTERNAL_ROOTS: &[&str] = &["std", "core", "alloc", "proc_macro"];

pub(crate) fn resolve_references(codebase: &mut Codebase, references: &[RustReference]) {
    let index = ResolutionIndex::new(codebase);
    let mut diagnostics = Vec::new();

    for reference in references {
        let outcome = index.resolve(reference);
        let (target, resolution) = match outcome {
            Resolution::Type(id) => (RelationshipTarget::Symbol(id), ResolutionStatus::Inferred),
            Resolution::External => continue,
            Resolution::Unresolved => {
                if needs_unresolved_diagnostic(reference) {
                    diagnostics.push(
                        Diagnostic::new(
                            DiagnosticSeverity::Warning,
                            format!(
                                "unresolved local reference `{}` from `{}`",
                                reference.path.display,
                                reference.location.file.display()
                            ),
                            Some(reference.location.clone()),
                        )
                        .with_code("analysis::unresolved-reference"),
                    );
                }
                (
                    RelationshipTarget::Unresolved(UnresolvedReference::new(
                        reference.path.display.clone(),
                    )),
                    ResolutionStatus::Unresolved,
                )
            }
            Resolution::Ambiguous(candidates) => {
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticSeverity::Warning,
                        format!(
                            "ambiguous reference `{}` from `{}`; candidates: {}",
                            reference.path.display,
                            reference.location.file.display(),
                            candidates.join(", ")
                        ),
                        Some(reference.location.clone()),
                    )
                    .with_code("analysis::ambiguous-reference"),
                );
                (
                    RelationshipTarget::Unresolved(UnresolvedReference::new(
                        reference.path.display.clone(),
                    )),
                    ResolutionStatus::Unresolved,
                )
            }
        };

        let evidence = RelationshipEvidence::new(reference.location.clone()).with_note(
            reference
                .note
                .clone()
                .unwrap_or_else(|| reference.path.display.clone()),
        );
        let relationship = Relationship::new(
            reference.source.clone(),
            target,
            reference.kind,
            resolution,
            Provenance::Syntax,
        )
        .with_evidence(evidence);
        if let Err(error) = codebase.add_relationship(relationship) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticSeverity::Error,
                    format!("could not add resolved reference: {error}"),
                    Some(reference.location.clone()),
                )
                .with_code("analysis::model-invariant"),
            );
        }
    }

    for diagnostic in diagnostics {
        codebase.add_diagnostic(diagnostic);
    }
}

enum Resolution {
    Type(SymbolId),
    Unresolved,
    Ambiguous(Vec<String>),
    External,
}

struct ResolutionIndex {
    types_by_qualified: BTreeMap<String, Vec<SymbolId>>,
    names_by_id: BTreeMap<SymbolId, String>,
    crate_aliases: BTreeMap<String, String>,
}

impl ResolutionIndex {
    fn new(codebase: &Codebase) -> Self {
        let mut types_by_qualified: BTreeMap<String, Vec<SymbolId>> = BTreeMap::new();
        let mut names_by_id = BTreeMap::new();
        for declaration in codebase.types() {
            types_by_qualified
                .entry(declaration.qualified_name.clone())
                .or_default()
                .push(declaration.id.clone());
            names_by_id.insert(declaration.id.clone(), declaration.qualified_name.clone());
        }
        for ids in types_by_qualified.values_mut() {
            ids.sort();
            ids.dedup();
        }

        let mut crate_aliases = BTreeMap::new();
        for module in codebase.modules() {
            if module.parent.is_none() {
                crate_aliases.insert(
                    normalize_crate_identifier(&module.qualified_name),
                    module.qualified_name.clone(),
                );
                crate_aliases.insert(
                    normalize_crate_identifier(&module.name),
                    module.qualified_name.clone(),
                );
            }
        }

        Self {
            types_by_qualified,
            names_by_id,
            crate_aliases,
        }
    }

    fn resolve(&self, reference: &RustReference) -> Resolution {
        let path = &reference.path;
        if path.root == RustPathRoot::Unresolved {
            return Resolution::Unresolved;
        }
        if path.segments.is_empty() {
            return Resolution::External;
        }

        let first = path.segments[0].as_str();
        if first == "Self"
            || reference
                .generic_parameters
                .iter()
                .any(|name| name == first)
        {
            return Resolution::External;
        }
        if path.root == RustPathRoot::Relative && is_primitive(first) {
            return Resolution::External;
        }

        let mut candidates = BTreeSet::new();
        let mut external_signal = path.root == RustPathRoot::Global;

        for candidate in self.direct_candidates(reference, path) {
            self.add_candidate(&candidate, &mut candidates);
        }

        if path.root == RustPathRoot::Relative {
            for import in &reference.imports {
                match import {
                    RustImport::Named {
                        alias,
                        path: import,
                    } if alias == first => {
                        for base in self.direct_candidates(reference, import) {
                            if is_known_external_path(&base) {
                                external_signal = true;
                            }
                            let candidate = if path.segments.len() == 1 {
                                base
                            } else {
                                format!("{base}::{}", path.segments[1..].join("::"))
                            };
                            self.add_candidate(&candidate, &mut candidates);
                        }
                    }
                    RustImport::Glob { path: import } => {
                        for base in self.direct_candidates(reference, import) {
                            if is_known_external_path(&base) {
                                external_signal = true;
                            }
                            self.add_candidate(
                                &format!("{base}::{}", path.segments.join("::")),
                                &mut candidates,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }

        if candidates.len() == 1 {
            return Resolution::Type(candidates.into_iter().next().expect("one candidate"));
        }
        if candidates.len() > 1 {
            return Resolution::Ambiguous(
                candidates
                    .into_iter()
                    .map(|id| {
                        self.names_by_id
                            .get(&id)
                            .cloned()
                            .unwrap_or_else(|| id.as_str().to_owned())
                    })
                    .collect(),
            );
        }

        if external_signal
            || is_known_external_path(&path.display)
            || (path.root == RustPathRoot::Relative
                && path.segments.len() == 1
                && KNOWN_EXTERNAL_TYPES.contains(&first))
        {
            Resolution::External
        } else {
            Resolution::Unresolved
        }
    }

    fn add_candidate(&self, qualified_name: &str, candidates: &mut BTreeSet<SymbolId>) {
        if let Some(ids) = self.types_by_qualified.get(qualified_name) {
            candidates.extend(ids.iter().cloned());
        }
    }

    fn direct_candidates(&self, reference: &RustReference, path: &RustTypePath) -> Vec<String> {
        let modules = module_qualified(&reference.crate_key, &reference.module_path);
        match &path.root {
            RustPathRoot::Crate => vec![join_qualified(&reference.crate_key, &path.segments)],
            RustPathRoot::SelfModule => vec![join_qualified(&modules, &path.segments)],
            RustPathRoot::Super(count) => {
                if *count > reference.module_path.len() {
                    return Vec::new();
                }
                let end = reference.module_path.len() - count;
                let base = module_qualified(&reference.crate_key, &reference.module_path[..end]);
                vec![join_qualified(&base, &path.segments)]
            }
            RustPathRoot::Global => Vec::new(),
            RustPathRoot::Unresolved => Vec::new(),
            RustPathRoot::Relative => {
                if path.segments.is_empty() {
                    return Vec::new();
                }
                let mut candidates = Vec::new();
                if let Some(crate_key) = self
                    .crate_aliases
                    .get(&normalize_crate_identifier(&path.segments[0]))
                {
                    candidates.push(join_qualified(crate_key, &path.segments[1..]));
                }
                candidates.push(join_qualified(&modules, &path.segments));
                candidates.sort();
                candidates.dedup();
                candidates
            }
        }
    }
}

fn module_qualified(crate_key: &str, module_path: &[String]) -> String {
    join_qualified(crate_key, module_path)
}

fn join_qualified(base: &str, segments: &[String]) -> String {
    let mut qualified = base.to_owned();
    for segment in segments {
        qualified.push_str("::");
        qualified.push_str(segment);
    }
    qualified
}

fn normalize_crate_identifier(name: &str) -> String {
    name.replace('-', "_")
}

fn is_primitive(name: &str) -> bool {
    PRIMITIVE_TYPES.contains(&name)
}

fn is_known_external_path(path: &str) -> bool {
    let first = path
        .trim_start_matches("::")
        .split("::")
        .next()
        .unwrap_or("");
    KNOWN_EXTERNAL_ROOTS.contains(&first)
}

fn needs_unresolved_diagnostic(reference: &RustReference) -> bool {
    matches!(
        reference.path.root,
        RustPathRoot::Crate
            | RustPathRoot::SelfModule
            | RustPathRoot::Super(_)
            | RustPathRoot::Unresolved
    )
}
