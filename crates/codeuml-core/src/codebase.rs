use std::collections::BTreeSet;

use crate::{
    AnalysisMetadata, Diagnostic, ModelError, Module, Relationship, RelationshipTarget,
    ResolutionStatus, SourceFile, SymbolId, TypeDecl,
};

/// The merged, language-neutral result of an analysis run.
///
/// Collections are private so declarations, members, and relationships can be
/// inserted through validating methods. The model keeps unresolved
/// relationships instead of silently dropping them.
#[derive(Clone, Debug)]
pub struct Codebase {
    name: String,
    files: Vec<SourceFile>,
    modules: Vec<Module>,
    types: Vec<TypeDecl>,
    relationships: Vec<Relationship>,
    diagnostics: Vec<Diagnostic>,
    analysis: AnalysisMetadata,
    declaration_ids: BTreeSet<SymbolId>,
}

impl Codebase {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            files: Vec::new(),
            modules: Vec::new(),
            types: Vec::new(),
            relationships: Vec::new(),
            diagnostics: Vec::new(),
            analysis: AnalysisMetadata::default(),
            declaration_ids: BTreeSet::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    pub fn types(&self) -> &[TypeDecl] {
        &self.types
    }

    pub fn relationships(&self) -> &[Relationship] {
        &self.relationships
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn analysis(&self) -> &AnalysisMetadata {
        &self.analysis
    }

    pub fn analysis_mut(&mut self) -> &mut AnalysisMetadata {
        &mut self.analysis
    }

    /// Add a source file. Returns `false` when the path was already present.
    pub fn add_file(&mut self, file: SourceFile) -> bool {
        if self.files.iter().any(|existing| existing.path == file.path) {
            return false;
        }
        self.files.push(file);
        true
    }

    pub fn add_module(&mut self, module: Module) -> Result<(), ModelError> {
        if self.declaration_ids.contains(&module.id) {
            return Err(ModelError::DuplicateDeclaration(module.id));
        }
        self.declaration_ids.insert(module.id.clone());
        self.modules.push(module);
        Ok(())
    }

    /// Add a type declaration together with its members.
    ///
    /// The insertion is atomic: if any nested declaration ID is duplicate, no
    /// part of the type is added.
    pub fn add_type(&mut self, declaration: TypeDecl) -> Result<(), ModelError> {
        let mut new_ids = BTreeSet::new();
        self.collect_type_ids(&declaration, &mut new_ids)?;

        self.declaration_ids.extend(new_ids);
        self.types.push(declaration);
        Ok(())
    }

    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Add a relationship, merging it with an existing edge when identity
    /// `(source, target, kind)` matches.
    ///
    /// Returns `true` when a new edge was inserted and `false` when an
    /// observation was merged into an existing edge.
    pub fn add_relationship(&mut self, relationship: Relationship) -> Result<bool, ModelError> {
        if !self.declaration_ids.contains(&relationship.source) {
            return Err(ModelError::UnknownRelationshipSource(
                relationship.source.clone(),
            ));
        }

        self.validate_relationship_target(&relationship)?;

        if let Some(existing) = self
            .relationships
            .iter_mut()
            .find(|existing| existing.same_identity(&relationship))
        {
            existing.merge_observation(&relationship);
            return Ok(false);
        }

        self.relationships.push(relationship);
        Ok(true)
    }

    pub fn contains_declaration(&self, id: &SymbolId) -> bool {
        self.declaration_ids.contains(id)
    }

    pub fn definition_count(&self) -> usize {
        self.declaration_ids.len()
    }

    pub fn module(&self, id: &SymbolId) -> Option<&Module> {
        self.modules.iter().find(|module| &module.id == id)
    }

    pub fn type_by_id(&self, id: &SymbolId) -> Option<&TypeDecl> {
        self.types.iter().find(|declaration| &declaration.id == id)
    }

    pub fn types_in_module<'a>(
        &'a self,
        module: &'a SymbolId,
    ) -> impl Iterator<Item = &'a TypeDecl> + 'a {
        self.types
            .iter()
            .filter(move |declaration| declaration.module.as_ref() == Some(module))
    }

    pub fn relationships_from<'a>(
        &'a self,
        source: &'a SymbolId,
    ) -> impl Iterator<Item = &'a Relationship> + 'a {
        self.relationships
            .iter()
            .filter(move |relationship| &relationship.source == source)
    }

    pub fn relationships_to<'a>(
        &'a self,
        target: &'a SymbolId,
    ) -> impl Iterator<Item = &'a Relationship> + 'a {
        self.relationships
            .iter()
            .filter(move |relationship| relationship.target.symbol() == Some(target))
    }

    fn collect_type_ids(
        &self,
        declaration: &TypeDecl,
        new_ids: &mut BTreeSet<SymbolId>,
    ) -> Result<(), ModelError> {
        self.insert_new_id(&declaration.id, new_ids)?;
        for field in &declaration.fields {
            self.insert_new_id(&field.id, new_ids)?;
        }
        for method in &declaration.methods {
            self.insert_new_id(&method.id, new_ids)?;
        }
        for variant in &declaration.variants {
            self.insert_new_id(&variant.id, new_ids)?;
            for field in &variant.fields {
                self.insert_new_id(&field.id, new_ids)?;
            }
        }
        Ok(())
    }

    fn insert_new_id(
        &self,
        id: &SymbolId,
        new_ids: &mut BTreeSet<SymbolId>,
    ) -> Result<(), ModelError> {
        if self.declaration_ids.contains(id) || !new_ids.insert(id.clone()) {
            return Err(ModelError::DuplicateDeclaration(id.clone()));
        }
        Ok(())
    }

    fn validate_relationship_target(&self, relationship: &Relationship) -> Result<(), ModelError> {
        match (&relationship.target, relationship.resolution) {
            (RelationshipTarget::Symbol(target), ResolutionStatus::Unresolved) => {
                Err(ModelError::InconsistentRelationship {
                    reason: format!("resolved target `{target}` cannot have unresolved status"),
                })
            }
            (RelationshipTarget::Unresolved(reference), ResolutionStatus::Inferred) => {
                Err(ModelError::InconsistentRelationship {
                    reason: format!(
                        "unresolved target `{}` cannot have inferred status",
                        reference.name
                    ),
                })
            }
            (RelationshipTarget::Unresolved(reference), ResolutionStatus::Resolved) => {
                Err(ModelError::InconsistentRelationship {
                    reason: format!(
                        "unresolved target `{}` cannot have resolved status",
                        reference.name
                    ),
                })
            }
            (RelationshipTarget::Symbol(target), _) => {
                if self.declaration_ids.contains(target) {
                    Ok(())
                } else {
                    Err(ModelError::UnknownRelationshipTarget(target.clone()))
                }
            }
            (RelationshipTarget::Unresolved(_), ResolutionStatus::Unresolved) => Ok(()),
        }
    }
}
