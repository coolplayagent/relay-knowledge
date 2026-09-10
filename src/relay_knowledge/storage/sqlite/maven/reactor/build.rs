//! Resolves reactor coordinates without consulting registries or package caches.

use std::collections::BTreeMap;

use super::super::{model::EffectivePom, pom_path::relative_pom_path};
use super::{Edge, MAX_EDGES, MAX_MODULES, Module};
use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareBuildTarget, SoftwareBuildTargetInput,
        SoftwareRelationship, SoftwareRelationshipInput,
    },
    identity::stable_hash64,
    storage::StorageError,
};

pub(super) fn facts(
    models: &[EffectivePom],
    version: GraphVersion,
) -> Result<(Vec<Module>, Vec<Edge>), StorageError> {
    if models.len() > MAX_MODULES {
        return Err(StorageError::CapacityExceeded(
            "Maven reactor module budget exceeded".into(),
        ));
    }
    let modules = models
        .iter()
        .map(|model| module(model, version))
        .collect::<Result<Vec<_>, _>>()?;
    let mut coordinates = BTreeMap::<String, Vec<usize>>::new();
    let paths = models
        .iter()
        .enumerate()
        .map(|(i, model)| (model.document.path.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    for (i, model) in models.iter().enumerate() {
        coordinates
            .entry(format!("{}:{}", model.group_id, model.artifact_id))
            .or_default()
            .push(i);
    }
    let mut edges = BTreeMap::new();
    for (i, model) in models.iter().enumerate() {
        for dependency in &model.dependencies {
            let coordinate = dependency.coordinate();
            let candidates = coordinates
                .get(&coordinate)
                .into_iter()
                .flatten()
                .copied()
                .filter(|index| {
                    dependency
                        .version
                        .as_deref()
                        .is_some_and(|version| !version.contains("${"))
                        && dependency.version == models[*index].version
                        && dependency.classifier.as_deref().unwrap_or("").is_empty()
                        && dependency.dep_type.as_deref().unwrap_or("jar")
                            == models[*index].packaging.as_deref().unwrap_or("jar")
                        && !coordinate.contains("${")
                })
                .collect::<Vec<_>>();
            let target = if candidates.len() == 1 {
                Some(&modules[candidates[0]])
            } else {
                None
            };
            let state = if target.is_some() {
                "resolved"
            } else if candidates.len() > 1 {
                "ambiguous"
            } else {
                "unresolved"
            };
            let hint = format!(
                "{}:{} [scope={}, type={}, classifier={}, optional={}, profile={}]",
                coordinate,
                dependency.version.as_deref().unwrap_or("?"),
                dependency.scope.as_deref().unwrap_or("compile"),
                dependency.dep_type.as_deref().unwrap_or("jar"),
                dependency.classifier.as_deref().unwrap_or(""),
                dependency.optional.as_deref().unwrap_or("false"),
                dependency.profile.as_deref().unwrap_or("default")
            );
            let mut edge = relationship(
                &modules[i],
                target,
                "depends_on",
                state,
                hint,
                (&dependency.source_path, dependency.line),
                version,
            )?;
            edge.dependency_scope = dependency.scope.clone().unwrap_or_else(|| "compile".into());
            edge.profile = dependency.profile.clone();
            insert_edge(&mut edges, edge)?;
        }
        for member in &model.modules {
            let path = relative_pom_path(&model.document.path, &member.value).map(|path| {
                if path == "pom.xml" || path.ends_with("/pom.xml") {
                    path
                } else {
                    format!("{path}/pom.xml")
                }
            });
            let target = path
                .as_deref()
                .and_then(|path| paths.get(path))
                .map(|index| &modules[*index]);
            let state = if target.is_some() {
                "resolved"
            } else {
                "unresolved"
            };
            insert_edge(
                &mut edges,
                relationship(
                    &modules[i],
                    target,
                    "aggregates",
                    state,
                    member.value.clone(),
                    (&model.document.path, member.line),
                    version,
                )?,
            )?;
        }
    }
    Ok((modules, edges.into_values().collect()))
}

fn insert_edge(edges: &mut BTreeMap<String, Edge>, edge: Edge) -> Result<(), StorageError> {
    if edges.len() >= MAX_EDGES && !edges.contains_key(&edge.relationship.relationship_id) {
        return Err(StorageError::CapacityExceeded(
            "Maven reactor edge budget exceeded".into(),
        ));
    }
    edges.insert(edge.relationship.relationship_id.clone(), edge);
    Ok(())
}

fn module(model: &EffectivePom, version: GraphVersion) -> Result<Module, StorageError> {
    let path = &model.document.path;
    let directory = path
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent)
        .to_owned();
    let mut target = SoftwareBuildTarget::new(SoftwareBuildTargetInput {
        repository_id: model.document.repository_id.clone(),
        source_scope: model.document.source_scope.clone(),
        ecosystem: "maven".into(),
        language_id: "jvm".into(),
        name: model.coordinate.clone(),
        kind: "reactor_module".into(),
        command: None,
        output_hint: Some(if directory.is_empty() {
            ".".into()
        } else {
            directory.clone()
        }),
        source_kind: "pom.xml".into(),
        evidence_path: path.clone(),
        evidence_line_range: RepositoryCodeRange {
            start: model.line,
            end: model.line,
        },
        confidence_basis_points: 10_000,
        created_graph_version: version,
    })
    .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    // Logical identity survives line, coordinate/version and snapshot changes; SQL keys also include scope.
    target.target_id = format!(
        "maven_module:{:016x}",
        stable_hash64(format!("{}\0{}", model.document.repository_id, path).as_bytes())
    );
    Ok(Module { target, directory })
}

fn relationship(
    source: &Module,
    target: Option<&Module>,
    kind: &str,
    state: &str,
    hint: String,
    evidence: (&str, u32),
    version: GraphVersion,
) -> Result<Edge, StorageError> {
    let (path, line) = evidence;
    let target_id = target
        .map(|module| module.target.target_id.clone())
        .unwrap_or_else(|| format!("maven_target:{:016x}", stable_hash64(hint.as_bytes())));
    let mut relationship = SoftwareRelationship::new(SoftwareRelationshipInput {
        repository_id: source.target.repository_id.clone(),
        source_scope: source.target.source_scope.clone(),
        relationship_kind: kind.into(),
        source_id: source.target.target_id.clone(),
        source_kind: "module".into(),
        target_id,
        target_kind: if target.is_some() {
            "module"
        } else {
            "artifact"
        }
        .into(),
        target_hint: Some(hint.clone()),
        resolution_state: state.into(),
        confidence_basis_points: 10_000,
        confidence_tier: "extracted".into(),
        evidence_path: path.into(),
        evidence_line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        created_graph_version: version,
    })
    .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    // Distinct scopes/profiles declared on one line must not overwrite one another.
    relationship.relationship_id = format!(
        "maven_edge:{:016x}",
        stable_hash64(format!("{}\0{hint}", relationship.relationship_id).as_bytes())
    );
    Ok(Edge {
        relationship,
        dependency_scope: String::new(),
        profile: None,
    })
}

#[cfg(test)]
#[path = "build_tests.rs"]
mod tests;
