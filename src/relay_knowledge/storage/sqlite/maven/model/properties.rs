use std::collections::BTreeMap;

use super::{super::property_interpolation::interpolate, EffectivePom, ParentPom, RawProfile};

pub(super) fn insert_declared_parent_properties(
    properties: &mut BTreeMap<String, String>,
    declared_parent: Option<&ParentPom>,
    resolved_parent: Option<&EffectivePom>,
) {
    let group_id = declared_parent
        .and_then(|parent| parent.group_id.as_ref())
        .map(|value| interpolate(&value.value, properties))
        .or_else(|| resolved_parent.map(|parent| parent.group_id.clone()));
    let artifact_id = declared_parent
        .and_then(|parent| parent.artifact_id.as_ref())
        .map(|value| interpolate(&value.value, properties))
        .or_else(|| resolved_parent.map(|parent| parent.artifact_id.clone()));
    let version = declared_parent
        .and_then(|parent| parent.version.as_ref())
        .map(|value| interpolate(&value.value, properties))
        .or_else(|| resolved_parent.and_then(|parent| parent.version.clone()));
    insert_parent_property(properties, "groupId", group_id);
    insert_parent_property(properties, "artifactId", artifact_id);
    insert_parent_property(properties, "version", version);
}

fn insert_parent_property(
    properties: &mut BTreeMap<String, String>,
    name: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        properties.insert(format!("project.parent.{name}"), value.clone());
        properties.insert(format!("pom.parent.{name}"), value);
    }
}

pub(super) fn resolved_profile_properties(
    base: &BTreeMap<String, String>,
    profile: &RawProfile,
) -> BTreeMap<String, String> {
    let mut profile_properties = base.clone();
    merge_profile_properties(&mut profile_properties, profile);
    profile_properties
}

pub(super) fn merge_profile_properties(
    properties: &mut BTreeMap<String, String>,
    profile: &RawProfile,
) {
    let mut merged = properties.clone();
    for (key, value) in &profile.properties {
        merged.insert(key.clone(), value.value.clone());
    }
    for (key, value) in &profile.properties {
        properties.insert(key.clone(), interpolate(&value.value, &merged));
    }
}

#[cfg(test)]
#[path = "properties_tests.rs"]
mod tests;
