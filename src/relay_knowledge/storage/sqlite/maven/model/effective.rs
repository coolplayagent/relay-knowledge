use std::collections::{BTreeMap, BTreeSet};

use super::super::support::interpolate;
use super::{
    EffectiveDependency, EffectiveGoal, EffectivePlugin, EffectivePluginExecution, EffectivePom,
    PomDocument, RawDependency, RawPlugin, RawPluginExecution, RawPom, TaggedValue,
};

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

pub(super) struct ProfilePluginContext<'a> {
    pub(super) profile: &'a str,
    pub(super) profile_properties: &'a BTreeMap<String, String>,
    pub(super) profile_management: &'a BTreeMap<String, RawPlugin>,
    pub(super) default_properties: &'a BTreeMap<String, String>,
    pub(super) default_management: &'a BTreeMap<String, RawPlugin>,
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

pub(super) fn push_profile_plugin_variant(
    plugins: &mut Vec<EffectivePlugin>,
    plugin: &RawPlugin,
    context: &ProfilePluginContext<'_>,
) {
    let Some(profile_plugin) = effective_plugin(
        plugin,
        Some(context.profile.to_owned()),
        context.profile_properties,
        context.profile_management,
    ) else {
        return;
    };
    if let Some(default_plugin) = effective_plugin(
        plugin,
        None,
        context.default_properties,
        context.default_management,
    ) {
        if plugin_values_match(&profile_plugin, &default_plugin) {
            return;
        }
    }
    push_or_merge_plugin(plugins, profile_plugin);
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

fn plugin_values_match(left: &EffectivePlugin, right: &EffectivePlugin) -> bool {
    left.coordinate == right.coordinate
        && left.version == right.version
        && left.executions.len() == right.executions.len()
        && left
            .executions
            .iter()
            .zip(&right.executions)
            .all(|(left, right)| plugin_execution_values_match(left, right))
}

fn plugin_execution_values_match(
    left: &EffectivePluginExecution,
    right: &EffectivePluginExecution,
) -> bool {
    left.id == right.id
        && left.phase == right.phase
        && left.goals.len() == right.goals.len()
        && left
            .goals
            .iter()
            .zip(&right.goals)
            .all(|(left, right)| left.value == right.value)
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

pub(super) fn effective_plugin(
    plugin: &RawPlugin,
    profile: Option<String>,
    properties: &BTreeMap<String, String>,
    management: &BTreeMap<String, RawPlugin>,
) -> Option<EffectivePlugin> {
    let group_id = plugin
        .group_id
        .as_ref()
        .map(|value| interpolate(&value.value, properties))
        .unwrap_or_else(|| "org.apache.maven.plugins".to_owned());
    let artifact_id = plugin
        .artifact_id
        .as_ref()
        .map(|value| interpolate(&value.value, properties))?;
    let coordinate = format!("{group_id}:{artifact_id}");
    let managed = management.get(&coordinate);
    let version = plugin
        .version
        .as_ref()
        .or_else(|| managed.and_then(|plugin| plugin.version.as_ref()))
        .map(|value| interpolate(&value.value, properties));
    let inherited = raw_plugin_inherited(plugin, properties);
    let executions = effective_plugin_executions(plugin, managed, properties);

    Some(EffectivePlugin {
        artifact_id,
        version,
        executions,
        line: plugin.line,
        source_path: plugin.source_path.clone(),
        coordinate,
        profile,
        inherited,
    })
}

pub(super) fn raw_plugin_inherited(
    plugin: &RawPlugin,
    properties: &BTreeMap<String, String>,
) -> bool {
    plugin
        .inherited
        .as_ref()
        .map(|value| !interpolate(&value.value, properties).eq_ignore_ascii_case("false"))
        .unwrap_or(true)
}

pub(super) fn raw_plugin_execution_inherited(
    execution: &RawPluginExecution,
    properties: &BTreeMap<String, String>,
) -> bool {
    execution
        .inherited
        .as_ref()
        .map(|value| !interpolate(&value.value, properties).eq_ignore_ascii_case("false"))
        .unwrap_or(true)
}

fn effective_plugin_executions(
    plugin: &RawPlugin,
    managed: Option<&RawPlugin>,
    properties: &BTreeMap<String, String>,
) -> Vec<EffectivePluginExecution> {
    let mut executions = Vec::new();
    let mut merged_ids = BTreeSet::new();
    if let Some(managed) = managed {
        for managed_execution in &managed.executions {
            let managed_id = plugin_execution_key(managed_execution, properties);
            let matching_child = plugin
                .executions
                .iter()
                .find(|execution| plugin_execution_key(execution, properties) == managed_id);
            if let Some(child_execution) = matching_child {
                merged_ids.insert(managed_id);
                executions.push(merge_plugin_execution(
                    managed,
                    managed_execution,
                    plugin,
                    child_execution,
                    properties,
                ));
            } else {
                executions.push(effective_plugin_execution(
                    managed_execution,
                    properties,
                    &managed.source_path,
                ));
            }
        }
    }
    for execution in &plugin.executions {
        let execution_id = plugin_execution_key(execution, properties);
        if merged_ids.contains(&execution_id) {
            continue;
        }
        executions.push(effective_plugin_execution(
            execution,
            properties,
            &plugin.source_path,
        ));
    }
    executions
}

fn plugin_execution_id(
    execution: &RawPluginExecution,
    properties: &BTreeMap<String, String>,
) -> Option<String> {
    execution
        .id
        .as_ref()
        .map(|value| interpolate(&value.value, properties))
}

fn plugin_execution_key(
    execution: &RawPluginExecution,
    properties: &BTreeMap<String, String>,
) -> String {
    plugin_execution_id(execution, properties).unwrap_or_else(|| "default".to_owned())
}

fn merge_plugin_execution(
    managed_plugin: &RawPlugin,
    managed_execution: &RawPluginExecution,
    child_plugin: &RawPlugin,
    child_execution: &RawPluginExecution,
    properties: &BTreeMap<String, String>,
) -> EffectivePluginExecution {
    let managed =
        effective_plugin_execution(managed_execution, properties, &managed_plugin.source_path);
    let child = effective_plugin_execution(child_execution, properties, &child_plugin.source_path);
    let goals = if child.goals.is_empty() {
        managed.goals
    } else {
        child.goals
    };
    EffectivePluginExecution {
        id: child.id.or(managed.id),
        phase: child.phase.or(managed.phase),
        goals,
        line: child.line,
        source_path: child.source_path,
        inherited: child.inherited,
    }
}

fn effective_plugin_execution(
    execution: &RawPluginExecution,
    properties: &BTreeMap<String, String>,
    source_path: &str,
) -> EffectivePluginExecution {
    EffectivePluginExecution {
        id: plugin_execution_id(execution, properties),
        phase: execution
            .phase
            .as_ref()
            .map(|value| interpolate(&value.value, properties)),
        goals: execution
            .goals
            .iter()
            .map(|goal| EffectiveGoal {
                value: interpolate(&goal.value, properties),
                line: goal.line,
                source_path: source_path.to_owned(),
            })
            .collect(),
        line: execution.line,
        source_path: source_path.to_owned(),
        inherited: raw_plugin_execution_inherited(execution, properties),
    }
}

pub(super) fn inherited_plugin_for_child(
    plugin: &EffectivePlugin,
    properties: &BTreeMap<String, String>,
    management: &BTreeMap<String, RawPlugin>,
) -> Option<EffectivePlugin> {
    if !plugin.inherited || plugin.profile.is_some() {
        return None;
    }
    let mut inherited = plugin.clone();
    inherited.executions.retain(|execution| execution.inherited);
    Some(apply_plugin_management(inherited, properties, management))
}

fn apply_plugin_management(
    mut plugin: EffectivePlugin,
    properties: &BTreeMap<String, String>,
    management: &BTreeMap<String, RawPlugin>,
) -> EffectivePlugin {
    let Some(managed) = management.get(&plugin.coordinate) else {
        return plugin;
    };
    if plugin.version.is_none() {
        plugin.version = managed
            .version
            .as_ref()
            .map(|value| interpolate(&value.value, properties));
    }
    if let Some(managed_plugin) = effective_plugin(managed, None, properties, &BTreeMap::new()) {
        for execution in managed_plugin.executions {
            push_managed_execution(&mut plugin.executions, execution);
        }
    }
    plugin
}

fn push_managed_execution(
    executions: &mut Vec<EffectivePluginExecution>,
    execution: EffectivePluginExecution,
) {
    let id = effective_plugin_execution_key(&execution);
    let Some(existing) = executions
        .iter_mut()
        .find(|existing| effective_plugin_execution_key(existing) == id)
    else {
        executions.push(execution);
        return;
    };

    if existing.phase.is_none() {
        existing.phase = execution.phase;
    }
    if existing.goals.is_empty() {
        existing.goals = execution.goals;
    }
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

pub(super) fn dedupe_plugins(plugins: Vec<EffectivePlugin>) -> Vec<EffectivePlugin> {
    let mut deduped = Vec::new();
    for plugin in plugins {
        push_or_merge_plugin(&mut deduped, plugin);
    }
    deduped
}

pub(super) fn push_or_merge_plugin(plugins: &mut Vec<EffectivePlugin>, plugin: EffectivePlugin) {
    let key = effective_plugin_key(&plugin);
    let Some(existing) = plugins
        .iter_mut()
        .find(|existing| effective_plugin_key(existing) == key)
    else {
        plugins.push(plugin);
        return;
    };

    if plugin.version.is_some() {
        existing.version = plugin.version;
    }
    existing.line = plugin.line;
    existing.source_path = plugin.source_path;
    existing.inherited = plugin.inherited;
    existing.profile = plugin.profile;
    for execution in plugin.executions {
        push_or_merge_execution(&mut existing.executions, execution);
    }
}

fn effective_plugin_key(plugin: &EffectivePlugin) -> String {
    format!(
        "{}:{}",
        plugin.coordinate,
        plugin.profile.as_deref().unwrap_or_default()
    )
}

fn push_or_merge_execution(
    executions: &mut Vec<EffectivePluginExecution>,
    execution: EffectivePluginExecution,
) {
    let id = effective_plugin_execution_key(&execution);
    let Some(existing) = executions
        .iter_mut()
        .find(|existing| effective_plugin_execution_key(existing) == id)
    else {
        executions.push(execution);
        return;
    };

    if execution.phase.is_some() {
        existing.phase = execution.phase;
    }
    if !execution.goals.is_empty() {
        existing.goals = execution.goals;
    }
    existing.id = execution.id;
    existing.line = execution.line;
    existing.source_path = execution.source_path;
    existing.inherited = execution.inherited;
}

fn effective_plugin_execution_key(execution: &EffectivePluginExecution) -> &str {
    execution.id.as_deref().unwrap_or("default")
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

pub(super) fn raw_plugin_key(
    plugin: &RawPlugin,
    properties: &BTreeMap<String, String>,
) -> Option<String> {
    Some(format!(
        "{}:{}",
        plugin
            .group_id
            .as_ref()
            .map(|value| interpolate(&value.value, properties))
            .unwrap_or_else(|| "org.apache.maven.plugins".to_owned()),
        plugin
            .artifact_id
            .as_ref()
            .map(|value| interpolate(&value.value, properties))?
    ))
}
