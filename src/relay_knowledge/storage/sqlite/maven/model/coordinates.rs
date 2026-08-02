//! Effective Maven project coordinates and standard project property aliases.

use std::collections::BTreeMap;

use super::super::property_interpolation::interpolate;
use super::{EffectivePom, RawPom};

pub(super) struct ProjectCoordinates {
    pub(super) group_id: String,
    pub(super) artifact_id: String,
    pub(super) version: Option<String>,
}

pub(super) fn parent_properties(parent: &EffectivePom) -> BTreeMap<String, String> {
    let mut properties = parent.properties.clone();
    properties.insert("project.groupId".to_owned(), parent.group_id.clone());
    properties.insert("pom.groupId".to_owned(), parent.group_id.clone());
    properties.insert("project.artifactId".to_owned(), parent.artifact_id.clone());
    properties.insert("pom.artifactId".to_owned(), parent.artifact_id.clone());
    if let Some(version) = &parent.version {
        properties.insert("project.version".to_owned(), version.clone());
        properties.insert("pom.version".to_owned(), version.clone());
    }
    properties
}

pub(super) fn project_coordinates(
    raw: &RawPom,
    parent: Option<&EffectivePom>,
    properties: &BTreeMap<String, String>,
) -> ProjectCoordinates {
    let group_id = raw
        .group_id
        .as_ref()
        .or(raw
            .parent
            .as_ref()
            .and_then(|parent| parent.group_id.as_ref()))
        .map(|value| interpolate(&value.value, properties))
        .or_else(|| parent.map(|parent| parent.group_id.clone()))
        .unwrap_or_else(|| "unknown".to_owned());
    let artifact_id = raw
        .artifact_id
        .as_ref()
        .map(|value| interpolate(&value.value, properties))
        .unwrap_or_else(|| "unknown".to_owned());
    let version = raw
        .version
        .as_ref()
        .or(raw
            .parent
            .as_ref()
            .and_then(|parent| parent.version.as_ref()))
        .map(|value| interpolate(&value.value, properties))
        .or_else(|| parent.and_then(|parent| parent.version.clone()));
    ProjectCoordinates {
        group_id,
        artifact_id,
        version,
    }
}

pub(super) fn insert_project_properties(
    properties: &mut BTreeMap<String, String>,
    coordinates: &ProjectCoordinates,
) {
    properties.insert("project.groupId".to_owned(), coordinates.group_id.clone());
    properties.insert("pom.groupId".to_owned(), coordinates.group_id.clone());
    properties.insert(
        "project.artifactId".to_owned(),
        coordinates.artifact_id.clone(),
    );
    properties.insert("pom.artifactId".to_owned(), coordinates.artifact_id.clone());
    if let Some(version) = &coordinates.version {
        properties.insert("project.version".to_owned(), version.clone());
        properties.insert("pom.version".to_owned(), version.clone());
    }
}
