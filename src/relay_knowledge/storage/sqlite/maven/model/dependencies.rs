//! Effective dependency management, profile variants, and stable deduplication.

use std::collections::{BTreeMap, BTreeSet};

use super::super::property_interpolation::interpolate;
use super::{EffectiveDependency, PomDocument, RawDependency, TaggedValue};

pub(super) fn dependency_management_keys(
    dependencies: &[RawDependency],
    properties: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    dependencies
        .iter()
        .filter(|dependency| !raw_dependency_is_bom(dependency, properties))
        .filter_map(|dependency| raw_dependency_key(dependency, properties))
        .collect()
}

pub(super) fn push_dependency_management(
    management: &mut BTreeMap<String, RawDependency>,
    dependency: RawDependency,
    properties: &BTreeMap<String, String>,
) {
    if let Some(key) = raw_dependency_key(&dependency, properties) {
        management.insert(key, dependency);
    }
}

pub(super) fn push_imported_dependency_management(
    management: &mut BTreeMap<String, RawDependency>,
    dependency: RawDependency,
    properties: &BTreeMap<String, String>,
    protected_keys: &BTreeSet<String>,
    imported_keys: &mut BTreeSet<String>,
) {
    let Some(key) = raw_dependency_key(&dependency, properties) else {
        return;
    };
    if protected_keys.contains(&key) || imported_keys.contains(&key) {
        return;
    }
    management.insert(key.clone(), dependency);
    imported_keys.insert(key);
}

pub(super) fn resolved_management_dependency(
    dependency: &RawDependency,
    properties: &BTreeMap<String, String>,
) -> RawDependency {
    RawDependency {
        group_id: resolved_tagged(&dependency.group_id, properties),
        artifact_id: resolved_tagged(&dependency.artifact_id, properties),
        version: resolved_tagged(&dependency.version, properties),
        scope: resolved_tagged(&dependency.scope, properties),
        dep_type: resolved_tagged(&dependency.dep_type, properties),
        classifier: resolved_tagged(&dependency.classifier, properties),
        optional: resolved_tagged(&dependency.optional, properties),
        line: dependency.line,
    }
}

fn resolved_tagged(
    value: &Option<TaggedValue>,
    properties: &BTreeMap<String, String>,
) -> Option<TaggedValue> {
    value.as_ref().map(|value| TaggedValue {
        value: interpolate(&value.value, properties),
        line: value.line,
    })
}

pub(super) fn effective_dependency(
    dependency: &RawDependency,
    profile: Option<String>,
    properties: &BTreeMap<String, String>,
    management: &BTreeMap<String, RawDependency>,
    document: &PomDocument,
) -> Option<EffectiveDependency> {
    let group_id = dependency
        .group_id
        .as_ref()
        .map(|value| interpolate(&value.value, properties))?;
    let artifact_id = dependency
        .artifact_id
        .as_ref()
        .map(|value| interpolate(&value.value, properties))?;
    let managed = raw_dependency_key(dependency, properties).and_then(|key| management.get(&key));
    let version = dependency
        .version
        .as_ref()
        .or_else(|| managed.and_then(|dependency| dependency.version.as_ref()))
        .map(|value| interpolate(&value.value, properties));
    let scope = dependency
        .scope
        .as_ref()
        .or_else(|| managed.and_then(|dependency| dependency.scope.as_ref()))
        .map(|value| interpolate(&value.value, properties));
    let dep_type = dependency
        .dep_type
        .as_ref()
        .or_else(|| managed.and_then(|dependency| dependency.dep_type.as_ref()))
        .map(|value| interpolate(&value.value, properties));
    let classifier = dependency
        .classifier
        .as_ref()
        .or_else(|| managed.and_then(|dependency| dependency.classifier.as_ref()))
        .map(|value| interpolate(&value.value, properties));
    let optional = dependency
        .optional
        .as_ref()
        .or_else(|| managed.and_then(|dependency| dependency.optional.as_ref()))
        .map(|value| interpolate(&value.value, properties));

    Some(EffectiveDependency {
        group_id,
        artifact_id,
        version,
        scope,
        dep_type,
        classifier,
        optional,
        profile,
        line: dependency.line,
        source_file_id: document.file_id.clone(),
        source_path: document.path.clone(),
    })
}

pub(super) fn push_or_replace_dependency(
    dependencies: &mut Vec<EffectiveDependency>,
    dependency: EffectiveDependency,
) {
    let key = effective_dependency_key(&dependency);
    dependencies.retain(|existing| effective_dependency_key(existing) != key);
    dependencies.push(dependency);
}

pub(super) struct ProfileDependencyContext<'a> {
    pub(super) profile: &'a str,
    pub(super) profile_properties: &'a BTreeMap<String, String>,
    pub(super) profile_management: &'a BTreeMap<String, RawDependency>,
    pub(super) default_properties: &'a BTreeMap<String, String>,
    pub(super) default_management: &'a BTreeMap<String, RawDependency>,
    pub(super) document: &'a PomDocument,
}

pub(super) fn push_profile_dependency_variant(
    dependencies: &mut Vec<EffectiveDependency>,
    dependency: &RawDependency,
    context: &ProfileDependencyContext<'_>,
) {
    let Some(profile_dependency) = effective_dependency(
        dependency,
        Some(context.profile.to_owned()),
        context.profile_properties,
        context.profile_management,
        context.document,
    ) else {
        return;
    };
    if let Some(default_dependency) = effective_dependency(
        dependency,
        None,
        context.default_properties,
        context.default_management,
        context.document,
    ) {
        if dependency_values_match(&profile_dependency, &default_dependency) {
            return;
        }
    }
    push_or_replace_dependency(dependencies, profile_dependency);
}

fn dependency_values_match(left: &EffectiveDependency, right: &EffectiveDependency) -> bool {
    left.group_id == right.group_id
        && left.artifact_id == right.artifact_id
        && left.version == right.version
        && left.scope == right.scope
        && left.dep_type == right.dep_type
        && left.classifier == right.classifier
        && left.optional == right.optional
}

fn effective_dependency_key(dependency: &EffectiveDependency) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        dependency.group_id,
        dependency.artifact_id,
        dependency.dep_type.as_deref().unwrap_or("jar"),
        dependency.classifier.as_deref().unwrap_or_default(),
        dependency.profile.as_deref().unwrap_or_default()
    )
}

pub(super) fn dedupe_dependencies(
    dependencies: Vec<EffectiveDependency>,
) -> Vec<EffectiveDependency> {
    let mut deduped = Vec::new();
    for dependency in dependencies {
        push_or_replace_dependency(&mut deduped, dependency);
    }
    deduped
}

fn raw_dependency_key(
    dependency: &RawDependency,
    properties: &BTreeMap<String, String>,
) -> Option<String> {
    let dep_type = dependency
        .dep_type
        .as_ref()
        .map(|value| interpolate(&value.value, properties))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "jar".to_owned());
    let classifier = dependency
        .classifier
        .as_ref()
        .map(|value| interpolate(&value.value, properties))
        .unwrap_or_default();
    Some(format!(
        "{}:{}:{}:{}",
        dependency
            .group_id
            .as_ref()
            .map(|value| interpolate(&value.value, properties))?,
        dependency
            .artifact_id
            .as_ref()
            .map(|value| interpolate(&value.value, properties))?,
        dep_type,
        classifier
    ))
}

pub(super) fn raw_dependency_is_bom(
    dependency: &RawDependency,
    properties: &BTreeMap<String, String>,
) -> bool {
    dependency
        .dep_type
        .as_ref()
        .map(|value| interpolate(&value.value, properties))
        .as_deref()
        == Some("pom")
        && dependency
            .scope
            .as_ref()
            .map(|value| interpolate(&value.value, properties))
            .as_deref()
            == Some("import")
}
