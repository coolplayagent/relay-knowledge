use std::{borrow::Cow, collections::BTreeMap};

/// Effective-model load result and malformed-input preservation decision.
pub(in crate::storage::sqlite::maven) struct ResolvedPomLoad {
    pub(in crate::storage::sqlite::maven) models: Vec<EffectivePom>,
    pub(in crate::storage::sqlite::maven) preserve_existing_facts: bool,
}

/// Indexed POM source used without reading outside the authorized snapshot.
#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct PomDocument {
    pub(in crate::storage::sqlite::maven) repository_id: String,
    pub(in crate::storage::sqlite::maven) source_scope: String,
    pub(in crate::storage::sqlite::maven) file_id: String,
    pub(in crate::storage::sqlite::maven) path: String,
    pub(in crate::storage::sqlite::maven) content: String,
    pub(in crate::storage::sqlite::maven) byte_start: u64,
    pub(in crate::storage::sqlite::maven) byte_end: u64,
}

/// Repository-local Maven model after parent, profile, and management resolution.
#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct EffectivePom {
    pub(in crate::storage::sqlite::maven) document: PomDocument,
    pub(in crate::storage::sqlite::maven) group_id: String,
    pub(in crate::storage::sqlite::maven) artifact_id: String,
    pub(in crate::storage::sqlite::maven) version: Option<String>,
    pub(in crate::storage::sqlite::maven) coordinate: String,
    pub(in crate::storage::sqlite::maven) packaging: Option<String>,
    pub(in crate::storage::sqlite::maven) modules: Vec<TaggedValue>,
    pub(in crate::storage::sqlite::maven) profiles: Vec<EffectiveProfile>,
    pub(in crate::storage::sqlite::maven) plugins: Vec<EffectivePlugin>,
    pub(in crate::storage::sqlite::maven) dependencies: Vec<EffectiveDependency>,
    pub(in crate::storage::sqlite::maven) languages: Vec<&'static str>,
    pub(in crate::storage::sqlite::maven) line: u32,
    pub(in crate::storage::sqlite::maven) dependency_management: BTreeMap<String, RawDependency>,
    pub(in crate::storage::sqlite::maven) plugin_management: BTreeMap<String, RawPlugin>,
    pub(in crate::storage::sqlite::maven) properties: BTreeMap<String, String>,
}

impl EffectivePom {
    pub(in crate::storage::sqlite::maven) fn packaging_phase(&self) -> &str {
        match self.packaging.as_deref() {
            Some("pom") => "validate",
            _ => "package",
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct EffectiveProfile {
    pub(in crate::storage::sqlite::maven) id: String,
    pub(in crate::storage::sqlite::maven) line: u32,
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct EffectivePlugin {
    pub(in crate::storage::sqlite::maven) artifact_id: String,
    pub(in crate::storage::sqlite::maven) version: Option<String>,
    pub(in crate::storage::sqlite::maven) executions: Vec<EffectivePluginExecution>,
    pub(in crate::storage::sqlite::maven) line: u32,
    pub(in crate::storage::sqlite::maven) source_path: String,
    pub(in crate::storage::sqlite::maven) coordinate: String,
    pub(in crate::storage::sqlite::maven) profile: Option<String>,
    pub(in crate::storage::sqlite::maven) inherited: bool,
}

impl EffectivePlugin {
    pub(in crate::storage::sqlite::maven) fn prefix(&self) -> String {
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

    pub(in crate::storage::sqlite::maven) fn scoped_name(&self, name: &str) -> String {
        self.profile
            .as_ref()
            .map(|profile| format!("profile:{profile}:{name}"))
            .unwrap_or_else(|| name.to_owned())
    }

    pub(in crate::storage::sqlite::maven) fn command(&self, target: &str) -> String {
        self.profile
            .as_ref()
            .map(|profile| format!("mvn -P{profile} {target}"))
            .unwrap_or_else(|| format!("mvn {target}"))
    }
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct EffectivePluginExecution {
    pub(in crate::storage::sqlite::maven) id: Option<String>,
    pub(in crate::storage::sqlite::maven) phase: Option<String>,
    pub(in crate::storage::sqlite::maven) goals: Vec<EffectiveGoal>,
    pub(in crate::storage::sqlite::maven) line: u32,
    pub(in crate::storage::sqlite::maven) source_path: String,
    pub(in crate::storage::sqlite::maven) inherited: bool,
}

impl EffectivePluginExecution {
    pub(in crate::storage::sqlite::maven) fn name(&self) -> Cow<'_, str> {
        self.id.as_deref().map(Cow::Borrowed).unwrap_or_else(|| {
            Cow::Owned(self.phase.clone().unwrap_or_else(|| "default".to_owned()))
        })
    }

    pub(in crate::storage::sqlite::maven) fn command(
        &self,
        plugin: &EffectivePlugin,
    ) -> Option<String> {
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
pub(in crate::storage::sqlite::maven) struct EffectiveGoal {
    pub(in crate::storage::sqlite::maven) value: String,
    pub(in crate::storage::sqlite::maven) line: u32,
    pub(in crate::storage::sqlite::maven) source_path: String,
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct EffectiveDependency {
    pub(in crate::storage::sqlite::maven) group_id: String,
    pub(in crate::storage::sqlite::maven) artifact_id: String,
    pub(in crate::storage::sqlite::maven) version: Option<String>,
    pub(in crate::storage::sqlite::maven) scope: Option<String>,
    pub(in crate::storage::sqlite::maven) dep_type: Option<String>,
    pub(in crate::storage::sqlite::maven) classifier: Option<String>,
    pub(in crate::storage::sqlite::maven) optional: Option<String>,
    pub(in crate::storage::sqlite::maven) profile: Option<String>,
    pub(in crate::storage::sqlite::maven) line: u32,
    pub(in crate::storage::sqlite::maven) source_file_id: String,
    pub(in crate::storage::sqlite::maven) source_path: String,
}

impl EffectiveDependency {
    pub(in crate::storage::sqlite::maven) fn coordinate(&self) -> String {
        format!("{}:{}", self.group_id, self.artifact_id)
    }

    pub(in crate::storage::sqlite::maven) fn dependency_group(&self) -> String {
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

    pub(in crate::storage::sqlite::maven) fn excerpt(&self, package_name: &str) -> String {
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
pub(in crate::storage::sqlite::maven) struct RawPom {
    pub(in crate::storage::sqlite::maven) document: PomDocument,
    pub(in crate::storage::sqlite::maven) group_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) artifact_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) version: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) packaging: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) parent: Option<ParentPom>,
    pub(in crate::storage::sqlite::maven) properties: BTreeMap<String, TaggedValue>,
    pub(in crate::storage::sqlite::maven) modules: Vec<TaggedValue>,
    pub(in crate::storage::sqlite::maven) dependencies: Vec<RawDependency>,
    pub(in crate::storage::sqlite::maven) dependency_management: Vec<RawDependency>,
    pub(in crate::storage::sqlite::maven) plugins: Vec<RawPlugin>,
    pub(in crate::storage::sqlite::maven) plugin_management: Vec<RawPlugin>,
    pub(in crate::storage::sqlite::maven) profiles: Vec<RawProfile>,
}

impl RawPom {
    pub(in crate::storage::sqlite::maven) fn coordinate_hint(&self) -> Option<String> {
        let mut properties = self.local_properties();
        for profile in self
            .profiles
            .iter()
            .filter(|profile| profile.active_by_default)
        {
            super::properties::merge_profile_properties(&mut properties, profile);
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
            super::super::property_interpolation::interpolate(&group_id.value, &properties),
            super::super::property_interpolation::interpolate(&artifact_id.value, &properties),
            super::super::property_interpolation::interpolate(&version.value, &properties)
        ))
    }

    pub(in crate::storage::sqlite::maven) fn local_properties(&self) -> BTreeMap<String, String> {
        self.properties
            .iter()
            .map(|(key, value)| (key.clone(), value.value.clone()))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct ParentPom {
    pub(in crate::storage::sqlite::maven) group_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) artifact_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) version: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) relative_path: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) line: u32,
}

impl ParentPom {
    pub(in crate::storage::sqlite::maven) fn coordinate(
        &self,
        properties: &BTreeMap<String, String>,
    ) -> Option<String> {
        Some(format!(
            "{}:{}:{}",
            super::super::property_interpolation::interpolate(
                &self.group_id.as_ref()?.value,
                properties,
            ),
            super::super::property_interpolation::interpolate(
                &self.artifact_id.as_ref()?.value,
                properties,
            ),
            super::super::property_interpolation::interpolate(
                &self.version.as_ref()?.value,
                properties,
            )
        ))
    }
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct RawProfile {
    pub(in crate::storage::sqlite::maven) id: TaggedValue,
    pub(in crate::storage::sqlite::maven) active_by_default: bool,
    pub(in crate::storage::sqlite::maven) properties: BTreeMap<String, TaggedValue>,
    pub(in crate::storage::sqlite::maven) dependencies: Vec<RawDependency>,
    pub(in crate::storage::sqlite::maven) dependency_management: Vec<RawDependency>,
    pub(in crate::storage::sqlite::maven) plugins: Vec<RawPlugin>,
    pub(in crate::storage::sqlite::maven) plugin_management: Vec<RawPlugin>,
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct RawPlugin {
    pub(in crate::storage::sqlite::maven) group_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) artifact_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) version: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) inherited: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) executions: Vec<RawPluginExecution>,
    pub(in crate::storage::sqlite::maven) line: u32,
    pub(in crate::storage::sqlite::maven) source_path: String,
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct RawPluginExecution {
    pub(in crate::storage::sqlite::maven) id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) phase: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) inherited: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) goals: Vec<TaggedValue>,
    pub(in crate::storage::sqlite::maven) line: u32,
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct RawDependency {
    pub(in crate::storage::sqlite::maven) group_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) artifact_id: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) version: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) scope: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) dep_type: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) classifier: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) optional: Option<TaggedValue>,
    pub(in crate::storage::sqlite::maven) line: u32,
}

#[derive(Debug, Clone)]
pub(in crate::storage::sqlite::maven) struct TaggedValue {
    pub(in crate::storage::sqlite::maven) value: String,
    pub(in crate::storage::sqlite::maven) line: u32,
}

#[cfg(test)]
#[path = "contracts_tests.rs"]
mod tests;
