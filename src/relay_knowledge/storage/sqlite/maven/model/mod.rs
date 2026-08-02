use std::collections::{BTreeMap, BTreeSet};

use crate::storage::StorageError;

use super::{pom_path::relative_pom_path, property_interpolation::interpolate};

mod contracts;
mod coordinates;
mod dependencies;
mod parse;
mod plugins;
mod properties;

#[cfg(test)]
mod coordinates_tests;
#[cfg(test)]
mod dependencies_tests;
#[cfg(test)]
mod plugins_tests;

pub(super) use contracts::{
    EffectiveDependency, EffectiveGoal, EffectivePlugin, EffectivePluginExecution, EffectivePom,
    EffectiveProfile, ParentPom, PomDocument, RawDependency, RawPlugin, RawPluginExecution, RawPom,
    RawProfile, ResolvedPomLoad, TaggedValue,
};

use coordinates::{insert_project_properties, parent_properties, project_coordinates};
use dependencies::{
    ProfileDependencyContext, dedupe_dependencies, dependency_management_keys,
    effective_dependency, push_dependency_management, push_imported_dependency_management,
    push_or_replace_dependency, push_profile_dependency_variant, raw_dependency_is_bom,
    resolved_management_dependency,
};
use plugins::{
    ProfilePluginContext, dedupe_plugins, effective_plugin, inherited_plugin_for_child,
    push_or_merge_plugin, push_profile_plugin_variant, raw_plugin_execution_inherited,
    raw_plugin_inherited, raw_plugin_key,
};

pub(super) const JVM_LANGUAGES: [&str; 3] = ["java", "kotlin", "scala"];

pub(super) fn resolve_effective_model_load(
    documents: Vec<PomDocument>,
) -> Result<ResolvedPomLoad, StorageError> {
    let mut raw_models = BTreeMap::<String, RawPom>::new();
    let mut preserve_existing_facts = false;
    for document in documents {
        let path = document.path.clone();
        let source_scope = document.source_scope.clone();
        match RawPom::parse(document) {
            Ok(Some(raw)) => {
                raw_models.insert(raw.document.path.clone(), raw);
            }
            Ok(None) => {}
            Err(StorageError::InvalidInput(error)) => {
                tracing::warn!(
                    source_scope = %source_scope,
                    path = %path,
                    error = %error,
                    "skipping malformed Maven pom.xml"
                );
                preserve_existing_facts = true;
            }
            Err(error) => return Err(error),
        }
    }
    let mut resolver = EffectiveResolver {
        raw_models,
        resolved: BTreeMap::new(),
        resolving: BTreeSet::new(),
    };
    Ok(ResolvedPomLoad {
        models: resolver.resolve_all()?,
        preserve_existing_facts,
    })
}

use properties::{
    insert_declared_parent_properties, merge_profile_properties, resolved_profile_properties,
};

struct EffectiveResolver {
    raw_models: BTreeMap<String, RawPom>,
    resolved: BTreeMap<String, EffectivePom>,
    resolving: BTreeSet<String>,
}

impl EffectiveResolver {
    fn resolve_all(&mut self) -> Result<Vec<EffectivePom>, StorageError> {
        let paths = self.raw_models.keys().cloned().collect::<Vec<_>>();
        let mut models = Vec::new();
        for path in paths {
            if let Some(model) = self.resolve_path(&path)? {
                models.push(model);
            }
        }
        models.sort_by(|left, right| left.document.path.cmp(&right.document.path));
        Ok(models)
    }

    fn resolve_path(&mut self, path: &str) -> Result<Option<EffectivePom>, StorageError> {
        if let Some(model) = self.resolved.get(path) {
            return Ok(Some(model.clone()));
        }
        if !self.raw_models.contains_key(path) || !self.resolving.insert(path.to_owned()) {
            return Ok(None);
        }
        let raw = self
            .raw_models
            .get(path)
            .cloned()
            .expect("path presence checked before resolution");
        let parent = self.resolve_parent(&raw)?;
        let model = self.build_effective(raw, parent)?;
        self.resolving.remove(path);
        self.resolved.insert(path.to_owned(), model.clone());
        Ok(Some(model))
    }

    fn resolve_parent(&mut self, raw: &RawPom) -> Result<Option<EffectivePom>, StorageError> {
        let Some(parent) = &raw.parent else {
            return Ok(None);
        };
        let parent_properties = raw.local_properties();
        if let Some(relative_path) = parent
            .relative_path
            .as_ref()
            .map(|value| value.value.as_str())
        {
            let relative_path = relative_path.trim();
            if relative_path.is_empty() {
                return Ok(None);
            }
            let Some(path) = relative_pom_path(&raw.document.path, relative_path) else {
                return Ok(None);
            };
            if let Some(model) =
                self.resolve_declared_parent_path(parent, &parent_properties, &path)?
            {
                return Ok(Some(model));
            }
            return Ok(None);
        } else if let Some(path) = relative_pom_path(&raw.document.path, "../pom.xml") {
            if let Some(model) =
                self.resolve_declared_parent_path(parent, &parent_properties, &path)?
            {
                return Ok(Some(model));
            }
        }

        let Some(coordinate) = parent.coordinate(&parent_properties) else {
            return Ok(None);
        };
        self.resolve_coordinate(&coordinate)
    }

    fn resolve_declared_parent_path(
        &mut self,
        parent: &ParentPom,
        properties: &BTreeMap<String, String>,
        path: &str,
    ) -> Result<Option<EffectivePom>, StorageError> {
        if !self.raw_models.contains_key(path) {
            return Ok(None);
        };
        let Some(expected_coordinate) = parent.coordinate(properties) else {
            return Ok(None);
        };
        let Some(candidate) = self.resolve_path(path)? else {
            return Ok(None);
        };
        if candidate.coordinate == expected_coordinate {
            Ok(Some(candidate))
        } else {
            Ok(None)
        }
    }

    fn resolve_coordinate(
        &mut self,
        coordinate: &str,
    ) -> Result<Option<EffectivePom>, StorageError> {
        if let Some(model) = self
            .resolved
            .values()
            .find(|model| model.coordinate == coordinate)
        {
            return Ok(Some(model.clone()));
        }

        let hinted_paths = self
            .raw_models
            .iter()
            .filter(|(_, candidate)| candidate.coordinate_hint().as_deref() == Some(coordinate))
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        for path in hinted_paths {
            if let Some(model) = self.resolve_path(&path)? {
                if model.coordinate == coordinate {
                    return Ok(Some(model));
                }
            }
        }

        let paths = self.raw_models.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            if self.resolving.contains(&path) {
                continue;
            }
            if let Some(model) = self.resolve_path(&path)? {
                if model.coordinate == coordinate {
                    return Ok(Some(model));
                }
            }
        }
        Ok(None)
    }

    fn merge_imported_bom_management(
        &mut self,
        management: &mut BTreeMap<String, RawDependency>,
        dependencies: &[RawDependency],
        properties: &BTreeMap<String, String>,
        protected_keys: &BTreeSet<String>,
        document: &PomDocument,
    ) -> Result<(), StorageError> {
        let mut imported_keys = BTreeSet::new();
        for dependency in dependencies {
            if !raw_dependency_is_bom(dependency, properties) {
                continue;
            }
            let Some(imported) =
                effective_dependency(dependency, None, properties, management, document)
            else {
                continue;
            };
            let Some(version) = imported.version.as_deref() else {
                continue;
            };
            let coordinate = format!("{}:{}:{version}", imported.group_id, imported.artifact_id);
            let Some(bom) = self.resolve_coordinate(&coordinate)? else {
                continue;
            };
            let empty_properties = BTreeMap::new();
            for dependency in bom.dependency_management.values() {
                let dependency = resolved_management_dependency(dependency, &bom.properties);
                push_imported_dependency_management(
                    management,
                    dependency,
                    &empty_properties,
                    protected_keys,
                    &mut imported_keys,
                );
            }
        }
        Ok(())
    }

    fn build_effective(
        &mut self,
        raw: RawPom,
        parent: Option<EffectivePom>,
    ) -> Result<EffectivePom, StorageError> {
        let mut base_properties = parent.as_ref().map(parent_properties).unwrap_or_default();
        for (key, value) in &raw.properties {
            base_properties.insert(key.clone(), value.value.clone());
        }
        insert_declared_parent_properties(
            &mut base_properties,
            raw.parent.as_ref(),
            parent.as_ref(),
        );
        let base_coordinates = project_coordinates(&raw, parent.as_ref(), &base_properties);
        insert_project_properties(&mut base_properties, &base_coordinates);

        let mut default_properties = base_properties.clone();
        for profile in raw
            .profiles
            .iter()
            .filter(|profile| profile.active_by_default)
        {
            merge_profile_properties(&mut default_properties, profile);
        }
        let coordinates = project_coordinates(&raw, parent.as_ref(), &default_properties);
        insert_project_properties(&mut default_properties, &coordinates);
        let group_id = coordinates.group_id;
        let artifact_id = coordinates.artifact_id;
        let version = coordinates.version;

        let mut base_dependency_management = parent
            .as_ref()
            .map(|parent| parent.dependency_management.clone())
            .unwrap_or_default();
        let direct_dependency_management_keys =
            dependency_management_keys(&raw.dependency_management, &base_properties);
        for dependency in raw.dependency_management.iter().cloned() {
            push_dependency_management(
                &mut base_dependency_management,
                dependency,
                &base_properties,
            );
        }
        self.merge_imported_bom_management(
            &mut base_dependency_management,
            &raw.dependency_management,
            &base_properties,
            &direct_dependency_management_keys,
            &raw.document,
        )?;
        let mut default_dependency_management = parent
            .as_ref()
            .map(|parent| parent.dependency_management.clone())
            .unwrap_or_default();
        let direct_default_dependency_management_keys =
            dependency_management_keys(&raw.dependency_management, &default_properties);
        for dependency in raw.dependency_management.iter().cloned() {
            push_dependency_management(
                &mut default_dependency_management,
                dependency,
                &default_properties,
            );
        }
        self.merge_imported_bom_management(
            &mut default_dependency_management,
            &raw.dependency_management,
            &default_properties,
            &direct_default_dependency_management_keys,
            &raw.document,
        )?;
        let parent_plugin_management: BTreeMap<String, RawPlugin> = parent
            .as_ref()
            .map(|parent| {
                parent
                    .plugin_management
                    .iter()
                    .filter(|(_, plugin)| raw_plugin_inherited(plugin, &parent.properties))
                    .map(|(key, plugin)| {
                        let mut plugin = plugin.clone();
                        plugin.executions.retain(|execution| {
                            raw_plugin_execution_inherited(execution, &parent.properties)
                        });
                        (key.clone(), plugin)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut base_plugin_management = parent_plugin_management.clone();
        for plugin in raw.plugin_management.iter().cloned() {
            if let Some(key) = raw_plugin_key(&plugin, &base_properties) {
                base_plugin_management.insert(key, plugin);
            }
        }
        let mut default_plugin_management = parent_plugin_management;
        for plugin in raw.plugin_management.iter().cloned() {
            if let Some(key) = raw_plugin_key(&plugin, &default_properties) {
                default_plugin_management.insert(key, plugin);
            }
        }
        for profile in raw
            .profiles
            .iter()
            .filter(|profile| profile.active_by_default)
        {
            let profile_properties = resolved_profile_properties(&default_properties, profile);
            let direct_profile_dependency_management_keys =
                dependency_management_keys(&profile.dependency_management, &profile_properties);
            for dependency in profile.dependency_management.iter().cloned() {
                push_dependency_management(
                    &mut default_dependency_management,
                    dependency,
                    &profile_properties,
                );
            }
            self.merge_imported_bom_management(
                &mut default_dependency_management,
                &profile.dependency_management,
                &profile_properties,
                &direct_profile_dependency_management_keys,
                &raw.document,
            )?;
            for plugin in profile.plugin_management.iter().cloned() {
                if let Some(key) = raw_plugin_key(&plugin, &profile_properties) {
                    default_plugin_management.insert(key, plugin);
                }
            }
        }

        let mut dependencies = parent
            .as_ref()
            .map(|parent| {
                parent
                    .dependencies
                    .iter()
                    .filter(|dependency| dependency.profile.is_none())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for dependency in &raw.dependency_management {
            if raw_dependency_is_bom(dependency, &default_properties) {
                if let Some(effective) = effective_dependency(
                    dependency,
                    None,
                    &default_properties,
                    &default_dependency_management,
                    &raw.document,
                ) {
                    push_or_replace_dependency(&mut dependencies, effective);
                }
            }
        }
        for dependency in &raw.dependencies {
            if let Some(effective) = effective_dependency(
                dependency,
                None,
                &default_properties,
                &default_dependency_management,
                &raw.document,
            ) {
                push_or_replace_dependency(&mut dependencies, effective);
            }
        }
        let mut profiles = Vec::new();
        let mut plugins: Vec<EffectivePlugin> = parent
            .as_ref()
            .map(|parent| {
                parent
                    .plugins
                    .iter()
                    .filter_map(|plugin| {
                        inherited_plugin_for_child(
                            plugin,
                            &default_properties,
                            &default_plugin_management,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        for plugin in &raw.plugins {
            if let Some(effective) = effective_plugin(
                plugin,
                None,
                &default_properties,
                &default_plugin_management,
            ) {
                push_or_merge_plugin(&mut plugins, effective);
            }
        }
        for profile in &raw.profiles {
            let profile_base_properties = if profile.active_by_default {
                &default_properties
            } else {
                &base_properties
            };
            let profile_base_dependency_management = if profile.active_by_default {
                &default_dependency_management
            } else {
                &base_dependency_management
            };
            let profile_base_plugin_management = if profile.active_by_default {
                &default_plugin_management
            } else {
                &base_plugin_management
            };
            let profile_id = interpolate(&profile.id.value, profile_base_properties);
            let profile_scope = (!profile.active_by_default).then(|| profile_id.clone());
            profiles.push(EffectiveProfile {
                id: profile_id.clone(),
                line: profile.id.line,
            });
            let profile_properties = resolved_profile_properties(profile_base_properties, profile);
            let mut profile_dependency_management = profile_base_dependency_management.clone();
            let direct_profile_dependency_management_keys =
                dependency_management_keys(&profile.dependency_management, &profile_properties);
            for dependency in profile.dependency_management.iter().cloned() {
                push_dependency_management(
                    &mut profile_dependency_management,
                    dependency,
                    &profile_properties,
                );
            }
            self.merge_imported_bom_management(
                &mut profile_dependency_management,
                &profile.dependency_management,
                &profile_properties,
                &direct_profile_dependency_management_keys,
                &raw.document,
            )?;
            if let Some(profile) = profile_scope.as_deref() {
                let dependency_context = ProfileDependencyContext {
                    profile,
                    profile_properties: &profile_properties,
                    profile_management: &profile_dependency_management,
                    default_properties: &default_properties,
                    default_management: &default_dependency_management,
                    document: &raw.document,
                };
                for dependency in &raw.dependencies {
                    push_profile_dependency_variant(
                        &mut dependencies,
                        dependency,
                        &dependency_context,
                    );
                }
                for dependency in &raw.dependency_management {
                    if raw_dependency_is_bom(dependency, &profile_properties) {
                        push_profile_dependency_variant(
                            &mut dependencies,
                            dependency,
                            &dependency_context,
                        );
                    }
                }
            }
            let mut profile_plugin_management = profile_base_plugin_management.clone();
            for plugin in profile.plugin_management.iter().cloned() {
                if let Some(key) = raw_plugin_key(&plugin, &profile_properties) {
                    profile_plugin_management.insert(key, plugin);
                }
            }
            if let Some(profile) = profile_scope.as_deref() {
                let plugin_context = ProfilePluginContext {
                    profile,
                    profile_properties: &profile_properties,
                    profile_management: &profile_plugin_management,
                    default_properties: &default_properties,
                    default_management: &default_plugin_management,
                };
                for plugin in &raw.plugins {
                    push_profile_plugin_variant(&mut plugins, plugin, &plugin_context);
                }
            }
            for dependency in &profile.dependencies {
                if let Some(effective) = effective_dependency(
                    dependency,
                    profile_scope.clone(),
                    &profile_properties,
                    &profile_dependency_management,
                    &raw.document,
                ) {
                    push_or_replace_dependency(&mut dependencies, effective);
                }
            }
            for dependency in &profile.dependency_management {
                if raw_dependency_is_bom(dependency, &profile_properties) {
                    if let Some(effective) = effective_dependency(
                        dependency,
                        profile_scope.clone(),
                        &profile_properties,
                        &profile_dependency_management,
                        &raw.document,
                    ) {
                        push_or_replace_dependency(&mut dependencies, effective);
                    }
                }
            }
            for plugin in &profile.plugins {
                if let Some(effective) = effective_plugin(
                    plugin,
                    profile_scope.clone(),
                    &profile_properties,
                    &profile_plugin_management,
                ) {
                    push_or_merge_plugin(&mut plugins, effective);
                }
            }
        }

        let packaging = raw
            .packaging
            .as_ref()
            .map(|value| interpolate(&value.value, &default_properties))
            .or_else(|| Some("jar".to_owned()));
        let coordinate = format!(
            "{}:{}{}",
            group_id,
            artifact_id,
            version
                .as_ref()
                .map(|version| format!(":{version}"))
                .unwrap_or_default()
        );
        let modules = raw
            .modules
            .iter()
            .map(|module| TaggedValue {
                value: interpolate(&module.value, &default_properties),
                line: module.line,
            })
            .collect();
        let line = raw
            .artifact_id
            .as_ref()
            .map(|value| value.line)
            .or_else(|| raw.parent.as_ref().map(|parent| parent.line))
            .unwrap_or(1);

        Ok(EffectivePom {
            document: raw.document,
            group_id,
            artifact_id,
            version,
            coordinate,
            packaging,
            modules,
            profiles,
            plugins: dedupe_plugins(plugins),
            dependencies: dedupe_dependencies(dependencies),
            languages: JVM_LANGUAGES.to_vec(),
            line,
            dependency_management: default_dependency_management,
            plugin_management: default_plugin_management,
            properties: default_properties,
        })
    }
}
