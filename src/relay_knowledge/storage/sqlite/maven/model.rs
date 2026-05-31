use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

use crate::storage::StorageError;

use super::support::{interpolate, relative_pom_path};

#[path = "model/effective.rs"]
mod effective;
#[path = "model/parse.rs"]
mod parse;

use effective::{
    ProfileDependencyContext, ProfilePluginContext, dedupe_dependencies, dedupe_plugins,
    dependency_management_keys, effective_dependency, effective_plugin, inherited_plugin_for_child,
    insert_project_properties, parent_properties, project_coordinates, push_dependency_management,
    push_imported_dependency_management, push_or_merge_plugin, push_or_replace_dependency,
    push_profile_dependency_variant, push_profile_plugin_variant, raw_dependency_is_bom,
    raw_plugin_execution_inherited, raw_plugin_inherited, raw_plugin_key,
    resolved_management_dependency,
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

pub(super) struct ResolvedPomLoad {
    pub(super) models: Vec<EffectivePom>,
    pub(super) preserve_existing_facts: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PomDocument {
    pub(super) repository_id: String,
    pub(super) source_scope: String,
    pub(super) file_id: String,
    pub(super) path: String,
    pub(super) content: String,
    pub(super) byte_start: u64,
    pub(super) byte_end: u64,
}

#[derive(Debug, Clone)]
pub(super) struct EffectivePom {
    pub(super) document: PomDocument,
    pub(super) group_id: String,
    pub(super) artifact_id: String,
    pub(super) version: Option<String>,
    pub(super) coordinate: String,
    pub(super) packaging: Option<String>,
    pub(super) modules: Vec<TaggedValue>,
    pub(super) profiles: Vec<EffectiveProfile>,
    pub(super) plugins: Vec<EffectivePlugin>,
    pub(super) dependencies: Vec<EffectiveDependency>,
    pub(super) languages: Vec<&'static str>,
    pub(super) line: u32,
    dependency_management: BTreeMap<String, RawDependency>,
    plugin_management: BTreeMap<String, RawPlugin>,
    properties: BTreeMap<String, String>,
}

impl EffectivePom {
    pub(super) fn packaging_phase(&self) -> &str {
        match self.packaging.as_deref() {
            Some("pom") => "validate",
            _ => "package",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct EffectiveProfile {
    pub(super) id: String,
    pub(super) line: u32,
}

#[derive(Debug, Clone)]
pub(super) struct EffectivePlugin {
    pub(super) artifact_id: String,
    pub(super) version: Option<String>,
    pub(super) executions: Vec<EffectivePluginExecution>,
    pub(super) line: u32,
    pub(super) source_path: String,
    pub(super) coordinate: String,
    pub(super) profile: Option<String>,
    inherited: bool,
}

impl EffectivePlugin {
    pub(super) fn prefix(&self) -> String {
        let artifact = self.artifact_id.as_str();
        if let Some(core) = artifact
            .strip_prefix("maven-")
            .and_then(|value| value.strip_suffix("-plugin"))
        {
            return core.to_owned();
        }
        if let Some(third_party) = artifact.strip_suffix("-maven-plugin") {
            return third_party.to_owned();
        }
        artifact
            .strip_suffix("-plugin")
            .unwrap_or(artifact)
            .to_owned()
    }

    pub(super) fn scoped_name(&self, name: &str) -> String {
        self.profile
            .as_ref()
            .map(|profile| format!("profile:{profile}:{name}"))
            .unwrap_or_else(|| name.to_owned())
    }

    pub(super) fn command(&self, target: &str) -> String {
        self.profile
            .as_ref()
            .map(|profile| format!("mvn -P{profile} {target}"))
            .unwrap_or_else(|| format!("mvn {target}"))
    }
}

#[derive(Debug, Clone)]
pub(super) struct EffectivePluginExecution {
    pub(super) id: Option<String>,
    pub(super) phase: Option<String>,
    pub(super) goals: Vec<EffectiveGoal>,
    pub(super) line: u32,
    pub(super) source_path: String,
    inherited: bool,
}

impl EffectivePluginExecution {
    pub(super) fn name(&self) -> Cow<'_, str> {
        self.id.as_deref().map(Cow::Borrowed).unwrap_or_else(|| {
            Cow::Owned(self.phase.clone().unwrap_or_else(|| "default".to_owned()))
        })
    }

    pub(super) fn command(&self, plugin: &EffectivePlugin) -> Option<String> {
        self.phase
            .as_ref()
            .map(|phase| plugin.command(phase))
            .or_else(|| {
                self.goals.first().map(|goal| {
                    let target = format!("{}:{}", plugin.prefix(), goal.value);
                    plugin.command(&target)
                })
            })
    }
}

#[derive(Debug, Clone)]
pub(super) struct EffectiveGoal {
    pub(super) value: String,
    pub(super) line: u32,
    pub(super) source_path: String,
}

#[derive(Debug, Clone)]
pub(super) struct EffectiveDependency {
    pub(super) group_id: String,
    pub(super) artifact_id: String,
    pub(super) version: Option<String>,
    scope: Option<String>,
    dep_type: Option<String>,
    classifier: Option<String>,
    optional: Option<String>,
    profile: Option<String>,
    pub(super) line: u32,
    pub(super) source_file_id: String,
    pub(super) source_path: String,
}

impl EffectiveDependency {
    pub(super) fn coordinate(&self) -> String {
        format!("{}:{}", self.group_id, self.artifact_id)
    }

    pub(super) fn dependency_group(&self) -> String {
        let base =
            if self.dep_type.as_deref() == Some("pom") && self.scope.as_deref() == Some("import") {
                "bom"
            } else {
                self.scope.as_deref().unwrap_or("compile")
            };
        match &self.profile {
            Some(profile) => format!("profile:{profile}:{base}"),
            None => base.to_owned(),
        }
    }

    pub(super) fn excerpt(&self, package_name: &str) -> String {
        let version = self.version.as_deref().unwrap_or("unversioned");
        let optional = self
            .optional
            .as_deref()
            .filter(|value| *value == "true")
            .map(|_| " optional")
            .unwrap_or_default();
        format!(
            "{package_name} {version}{} group={}",
            optional,
            self.dependency_group()
        )
    }
}

#[derive(Debug, Clone)]
struct RawPom {
    document: PomDocument,
    group_id: Option<TaggedValue>,
    artifact_id: Option<TaggedValue>,
    version: Option<TaggedValue>,
    packaging: Option<TaggedValue>,
    parent: Option<ParentPom>,
    properties: BTreeMap<String, TaggedValue>,
    modules: Vec<TaggedValue>,
    dependencies: Vec<RawDependency>,
    dependency_management: Vec<RawDependency>,
    plugins: Vec<RawPlugin>,
    plugin_management: Vec<RawPlugin>,
    profiles: Vec<RawProfile>,
}

impl RawPom {
    fn coordinate_hint(&self) -> Option<String> {
        let mut properties = self.local_properties();
        for profile in self
            .profiles
            .iter()
            .filter(|profile| profile.active_by_default)
        {
            merge_profile_properties(&mut properties, profile);
        }
        let group_id = self.group_id.as_ref().or(self
            .parent
            .as_ref()
            .and_then(|parent| parent.group_id.as_ref()))?;
        let artifact_id = self.artifact_id.as_ref()?;
        let version = self.version.as_ref().or(self
            .parent
            .as_ref()
            .and_then(|parent| parent.version.as_ref()))?;
        Some(format!(
            "{}:{}:{}",
            interpolate(&group_id.value, &properties),
            interpolate(&artifact_id.value, &properties),
            interpolate(&version.value, &properties)
        ))
    }

    fn local_properties(&self) -> BTreeMap<String, String> {
        self.properties
            .iter()
            .map(|(key, value)| (key.clone(), value.value.clone()))
            .collect()
    }
}

#[derive(Debug, Clone)]
struct ParentPom {
    group_id: Option<TaggedValue>,
    artifact_id: Option<TaggedValue>,
    version: Option<TaggedValue>,
    relative_path: Option<TaggedValue>,
    line: u32,
}

impl ParentPom {
    fn coordinate(&self, properties: &BTreeMap<String, String>) -> Option<String> {
        Some(format!(
            "{}:{}:{}",
            interpolate(&self.group_id.as_ref()?.value, properties),
            interpolate(&self.artifact_id.as_ref()?.value, properties),
            interpolate(&self.version.as_ref()?.value, properties)
        ))
    }
}

fn insert_declared_parent_properties(
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

fn resolved_profile_properties(
    base: &BTreeMap<String, String>,
    profile: &RawProfile,
) -> BTreeMap<String, String> {
    let mut profile_properties = base.clone();
    merge_profile_properties(&mut profile_properties, profile);
    profile_properties
}

fn merge_profile_properties(properties: &mut BTreeMap<String, String>, profile: &RawProfile) {
    let mut merged = properties.clone();
    for (key, value) in &profile.properties {
        merged.insert(key.clone(), value.value.clone());
    }
    for (key, value) in &profile.properties {
        properties.insert(key.clone(), interpolate(&value.value, &merged));
    }
}

#[derive(Debug, Clone)]
struct RawProfile {
    id: TaggedValue,
    active_by_default: bool,
    properties: BTreeMap<String, TaggedValue>,
    dependencies: Vec<RawDependency>,
    dependency_management: Vec<RawDependency>,
    plugins: Vec<RawPlugin>,
    plugin_management: Vec<RawPlugin>,
}

#[derive(Debug, Clone)]
struct RawPlugin {
    group_id: Option<TaggedValue>,
    artifact_id: Option<TaggedValue>,
    version: Option<TaggedValue>,
    inherited: Option<TaggedValue>,
    executions: Vec<RawPluginExecution>,
    line: u32,
    source_path: String,
}

#[derive(Debug, Clone)]
struct RawPluginExecution {
    id: Option<TaggedValue>,
    phase: Option<TaggedValue>,
    inherited: Option<TaggedValue>,
    goals: Vec<TaggedValue>,
    line: u32,
}

#[derive(Debug, Clone)]
struct RawDependency {
    group_id: Option<TaggedValue>,
    artifact_id: Option<TaggedValue>,
    version: Option<TaggedValue>,
    scope: Option<TaggedValue>,
    dep_type: Option<TaggedValue>,
    classifier: Option<TaggedValue>,
    optional: Option<TaggedValue>,
    line: u32,
}

#[derive(Debug, Clone)]
pub(super) struct TaggedValue {
    pub(super) value: String,
    pub(super) line: u32,
}

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
