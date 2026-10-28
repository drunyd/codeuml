use std::collections::{BTreeMap, BTreeSet};

use codeuml_core::{
    Codebase, Relationship, RelationshipKind, RelationshipTarget, SymbolId, TypeDecl,
};
use thiserror::Error;

use crate::model::{DiagramMember, DiagramMemberKind};
use crate::{
    DiagramEdge, DiagramEndpoint, DiagramNode, DiagramProjection, DiagramScope, ProjectionMetadata,
};

/// Project the analyzed model into diagram-ready nodes and edges.
pub fn project(
    codebase: &Codebase,
    scope: &DiagramScope,
) -> Result<DiagramProjection, ProjectionError> {
    let mut builder = ProjectionBuilder::new(codebase);
    let relationships = select_relationships(&mut builder, scope)?;

    let included_relationships = relationships.len();
    for relationship in relationships {
        builder.add_relationship(relationship);
    }

    let nodes = builder.nodes();
    let mut edges: Vec<DiagramEdge> = builder.edges.into_values().collect();
    edges.sort_by(|left, right| {
        (
            &left.source,
            &left.target,
            left.kind,
            left.resolution,
            left.provenance,
        )
            .cmp(&(
                &right.source,
                &right.target,
                right.kind,
                right.resolution,
                right.provenance,
            ))
    });

    let metadata = ProjectionMetadata {
        total_types: codebase.types().len(),
        included_types: nodes.len(),
        total_relationships: codebase.relationships().len(),
        included_relationships,
        omitted_relationships: codebase
            .relationships()
            .len()
            .saturating_sub(included_relationships),
        included_edges: edges.len(),
        boundary_edges: edges
            .iter()
            .filter(|edge| edge.source.is_boundary() || edge.target.is_boundary())
            .count(),
        unresolved_edges: edges
            .iter()
            .filter(|edge| {
                edge.source.boundary_symbol().is_none() && edge.source.is_boundary()
                    || edge.target.boundary_symbol().is_none() && edge.target.is_boundary()
            })
            .count(),
    };

    Ok(DiagramProjection {
        scope: scope.clone(),
        nodes,
        edges,
        diagnostics: codebase.diagnostics().to_vec(),
        metadata,
    })
}

fn select_relationships<'a>(
    builder: &mut ProjectionBuilder<'a>,
    scope: &DiagramScope,
) -> Result<Vec<&'a Relationship>, ProjectionError> {
    match scope {
        DiagramScope::Project => {
            builder.selected = builder.type_by_id.keys().cloned().collect();
            Ok(builder
                .codebase
                .relationships()
                .iter()
                .filter(|relationship| builder.included_source(relationship).is_some())
                .collect())
        }
        DiagramScope::Module { module } => {
            if builder.codebase.module(module).is_none() {
                return Err(ProjectionError::ModuleNotFound(module.clone()));
            }
            builder.selected = builder
                .type_by_id
                .values()
                .filter(|declaration| declaration.module.as_ref() == Some(module))
                .map(|declaration| declaration.id.clone())
                .collect();

            Ok(builder
                .codebase
                .relationships()
                .iter()
                .filter(|relationship| {
                    let source = builder.included_source(relationship);
                    let target = builder.target_owner(relationship);
                    source.is_some_and(|source| builder.selected.contains(source))
                        || target.is_some_and(|target| builder.selected.contains(target))
                })
                .collect())
        }
        DiagramScope::TypeNeighborhood { type_id, direction } => {
            if !builder.type_by_id.contains_key(type_id) {
                return Err(ProjectionError::TypeNotFound(type_id.clone()));
            }
            builder.selected.insert(type_id.clone());

            let mut selected_relationships = Vec::new();
            for relationship in builder.codebase.relationships() {
                let source = builder.included_source(relationship).cloned();
                let target = builder.target_owner(relationship).cloned();
                let outgoing = direction.includes_outgoing()
                    && source.as_ref().is_some_and(|source| source == type_id);
                let incoming = direction.includes_incoming()
                    && target.as_ref().is_some_and(|target| target == type_id);
                if !outgoing && !incoming {
                    continue;
                }
                selected_relationships.push(relationship);
                if outgoing {
                    if let Some(target) = target {
                        if builder.type_by_id.contains_key(&target) {
                            builder.selected.insert(target);
                        }
                    }
                }
                if incoming {
                    if let Some(source) = source {
                        if builder.type_by_id.contains_key(&source) {
                            builder.selected.insert(source);
                        }
                    }
                }
            }
            Ok(selected_relationships)
        }
    }
}

struct ProjectionBuilder<'a> {
    codebase: &'a Codebase,
    type_by_id: BTreeMap<SymbolId, &'a TypeDecl>,
    owner_by_member: BTreeMap<SymbolId, SymbolId>,
    selected: BTreeSet<SymbolId>,
    edges: BTreeMap<(DiagramEndpoint, DiagramEndpoint, RelationshipKind), DiagramEdge>,
}

impl<'a> ProjectionBuilder<'a> {
    fn new(codebase: &'a Codebase) -> Self {
        let mut type_by_id = BTreeMap::new();
        let mut owner_by_member = BTreeMap::new();
        for declaration in codebase.types() {
            type_by_id.insert(declaration.id.clone(), declaration);
            owner_by_member.insert(declaration.id.clone(), declaration.id.clone());
            for field in &declaration.fields {
                owner_by_member.insert(field.id.clone(), declaration.id.clone());
            }
            for method in &declaration.methods {
                owner_by_member.insert(method.id.clone(), declaration.id.clone());
            }
            for variant in &declaration.variants {
                owner_by_member.insert(variant.id.clone(), declaration.id.clone());
                for field in &variant.fields {
                    owner_by_member.insert(field.id.clone(), declaration.id.clone());
                }
            }
        }
        Self {
            codebase,
            type_by_id,
            owner_by_member,
            selected: BTreeSet::new(),
            edges: BTreeMap::new(),
        }
    }

    fn included_source(&self, relationship: &Relationship) -> Option<&SymbolId> {
        self.owner_by_member.get(&relationship.source)
    }

    fn target_owner(&self, relationship: &Relationship) -> Option<&SymbolId> {
        relationship
            .target
            .symbol()
            .and_then(|id| self.owner_by_member.get(id))
    }

    fn endpoint_for_symbol(&self, id: &SymbolId) -> DiagramEndpoint {
        match self.owner_by_member.get(id) {
            Some(owner) => self.endpoint_for_owner(owner),
            None => DiagramEndpoint::Boundary {
                symbol: Some(id.clone()),
                name: id.as_str().to_owned(),
            },
        }
    }

    fn endpoint_for_owner(&self, owner: &SymbolId) -> DiagramEndpoint {
        if self.selected.contains(owner) {
            DiagramEndpoint::Node(owner.clone())
        } else {
            DiagramEndpoint::Boundary {
                symbol: Some(owner.clone()),
                name: self
                    .type_by_id
                    .get(owner)
                    .map(|declaration| declaration.qualified_name.clone())
                    .unwrap_or_else(|| owner.as_str().to_owned()),
            }
        }
    }

    fn add_relationship(&mut self, relationship: &Relationship) {
        let Some(source_owner) = self.included_source(relationship).cloned() else {
            return;
        };
        let source = self.endpoint_for_owner(&source_owner);
        let target = match &relationship.target {
            RelationshipTarget::Symbol(id) => self.endpoint_for_symbol(id),
            RelationshipTarget::Unresolved(reference) => DiagramEndpoint::Boundary {
                symbol: None,
                name: reference.name.clone(),
            },
        };
        let key = (source.clone(), target.clone(), relationship.kind);
        if let Some(existing) = self.edges.get_mut(&key) {
            if !existing.source_members.contains(&relationship.source) {
                existing.source_members.push(relationship.source.clone());
                existing.source_members.sort();
            }
            for evidence in &relationship.evidence {
                if !existing.evidence.contains(evidence) {
                    existing.evidence.push(evidence.clone());
                }
            }
            existing.resolution = existing.resolution.max(relationship.resolution);
            existing.provenance = existing.provenance.max(relationship.provenance);
            return;
        }
        self.edges.insert(
            key,
            DiagramEdge {
                source,
                target,
                kind: relationship.kind,
                resolution: relationship.resolution,
                provenance: relationship.provenance,
                source_members: vec![relationship.source.clone()],
                evidence: relationship.evidence.clone(),
            },
        );
    }

    fn nodes(&self) -> Vec<DiagramNode> {
        let mut nodes: Vec<DiagramNode> = self
            .selected
            .iter()
            .filter_map(|id| self.type_by_id.get(id).copied())
            .map(|declaration| DiagramNode {
                id: declaration.id.clone(),
                name: declaration.name.clone(),
                qualified_name: declaration.qualified_name.clone(),
                kind: declaration.kind.clone(),
                module: declaration.module.clone(),
                location: declaration.location.clone(),
                visibility: declaration.visibility.clone(),
                alias_of: declaration.alias_of.clone(),
                field_count: declaration.fields.len(),
                method_count: declaration.methods.len(),
                variant_count: declaration.variants.len(),
                members: node_members(declaration),
            })
            .collect();
        nodes.sort_by(|left, right| {
            (&left.name, &left.qualified_name, &left.id).cmp(&(
                &right.name,
                &right.qualified_name,
                &right.id,
            ))
        });
        nodes
    }
}

fn node_members(declaration: &TypeDecl) -> Vec<DiagramMember> {
    let mut members = Vec::new();
    for field in &declaration.fields {
        let type_name = field.type_name.as_deref().unwrap_or("<unknown>");
        members.push(DiagramMember {
            kind: DiagramMemberKind::Field,
            text: format!("{}: {}", field.name, type_name),
        });
    }
    for variant in &declaration.variants {
        let fields = variant
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{}: {}",
                    field.name,
                    field.type_name.as_deref().unwrap_or("<unknown>")
                )
            })
            .collect::<Vec<_>>();
        let text = if fields.is_empty() {
            format!("variant {}", variant.name)
        } else {
            format!("variant {} {{ {} }}", variant.name, fields.join(", "))
        };
        members.push(DiagramMember {
            kind: DiagramMemberKind::Variant,
            text,
        });
    }
    for method in &declaration.methods {
        let parameters = method
            .parameters
            .iter()
            .map(|parameter| format!("{}: {}", parameter.name, parameter.type_name))
            .collect::<Vec<_>>()
            .join(", ");
        let return_type = method
            .return_type
            .as_ref()
            .map(|return_type| format!(" -> {return_type}"))
            .unwrap_or_default();
        members.push(DiagramMember {
            kind: DiagramMemberKind::Method,
            text: format!("fn {}({}){}", method.name, parameters, return_type),
        });
    }
    members
}

#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("module `{0}` was not found in the analyzed codebase")]
    ModuleNotFound(SymbolId),
    #[error("type `{0}` was not found in the analyzed codebase")]
    TypeNotFound(SymbolId),
}
