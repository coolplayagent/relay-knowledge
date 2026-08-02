//! Effective Maven plugin management, execution inheritance, and profile variants.

use std::collections::{BTreeMap, BTreeSet};

use super::super::property_interpolation::interpolate;
use super::{
    EffectiveGoal, EffectivePlugin, EffectivePluginExecution, RawPlugin, RawPluginExecution,
};

pub(super) struct ProfilePluginContext<'a> {
    pub(super) profile: &'a str,
    pub(super) profile_properties: &'a BTreeMap<String, String>,
    pub(super) profile_management: &'a BTreeMap<String, RawPlugin>,
    pub(super) default_properties: &'a BTreeMap<String, String>,
    pub(super) default_management: &'a BTreeMap<String, RawPlugin>,
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
