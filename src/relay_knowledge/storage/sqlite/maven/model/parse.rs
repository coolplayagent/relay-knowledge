use std::collections::BTreeMap;

use crate::storage::StorageError;

use super::super::xml::{XmlNode, parse_xml_document};
use super::{
    ParentPom, PomDocument, RawDependency, RawPlugin, RawPluginExecution, RawPom, RawProfile,
    TaggedValue,
};

impl RawPom {
    pub(super) fn parse(document: PomDocument) -> Result<Option<Self>, StorageError> {
        let Some(root) = parse_xml_document(&document.content)? else {
            return Ok(None);
        };
        if root.name != "project" {
            return Ok(None);
        }

        Ok(Some(Self {
            group_id: child_text(&root, "groupId"),
            artifact_id: child_text(&root, "artifactId"),
            version: child_text(&root, "version"),
            packaging: child_text(&root, "packaging"),
            parent: parse_parent(&root),
            properties: parse_properties(&root),
            modules: parse_modules(&root),
            dependencies: parse_dependencies(&root),
            dependency_management: root
                .child("dependencyManagement")
                .map(parse_dependencies)
                .unwrap_or_default(),
            plugins: parse_plugins(&root, &document.path),
            plugin_management: parse_plugin_management(&root, &document.path),
            profiles: parse_profiles(&root, &document.path),
            document,
        }))
    }
}

fn parse_parent(root: &XmlNode) -> Option<ParentPom> {
    let parent = root.child("parent")?;
    Some(ParentPom {
        group_id: child_text(parent, "groupId"),
        artifact_id: child_text(parent, "artifactId"),
        version: child_text(parent, "version"),
        relative_path: child_text_allow_empty(parent, "relativePath"),
        line: parent.child("artifactId")?.line,
    })
}

fn parse_properties(root: &XmlNode) -> BTreeMap<String, TaggedValue> {
    root.child("properties")
        .map(|properties| {
            properties
                .children()
                .iter()
                .filter_map(|child| {
                    let value = child.text.trim();
                    (!value.is_empty()).then(|| {
                        (
                            child.name.clone(),
                            TaggedValue {
                                value: value.to_owned(),
                                line: child.line,
                            },
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_modules(root: &XmlNode) -> Vec<TaggedValue> {
    root.child("modules")
        .map(|modules| {
            modules
                .children_named("module")
                .filter_map(tagged_node)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_profiles(root: &XmlNode, source_path: &str) -> Vec<RawProfile> {
    root.child("profiles")
        .map(|profiles| {
            profiles
                .children_named("profile")
                .filter_map(|profile| {
                    let id = child_text(profile, "id")?;
                    Some(RawProfile {
                        properties: parse_properties(profile),
                        dependencies: parse_dependencies(profile),
                        dependency_management: profile
                            .child("dependencyManagement")
                            .map(parse_dependencies)
                            .unwrap_or_default(),
                        plugins: parse_plugins(profile, source_path),
                        plugin_management: parse_plugin_management(profile, source_path),
                        active_by_default: profile
                            .child("activation")
                            .and_then(|activation| child_text(activation, "activeByDefault"))
                            .map(|value| value.value.eq_ignore_ascii_case("true"))
                            .unwrap_or(false),
                        id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_dependencies(root: &XmlNode) -> Vec<RawDependency> {
    root.child("dependencies")
        .map(|dependencies| {
            dependencies
                .children_named("dependency")
                .filter_map(|dependency| {
                    let group_id = child_text(dependency, "groupId");
                    let artifact_id = child_text(dependency, "artifactId");
                    (group_id.is_some() && artifact_id.is_some()).then(|| RawDependency {
                        line: group_id
                            .as_ref()
                            .or(artifact_id.as_ref())
                            .map(|value| value.line)
                            .unwrap_or(1),
                        group_id,
                        artifact_id,
                        version: child_text(dependency, "version"),
                        scope: child_text(dependency, "scope"),
                        dep_type: child_text(dependency, "type"),
                        classifier: child_text(dependency, "classifier"),
                        optional: child_text(dependency, "optional"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_plugins(root: &XmlNode, source_path: &str) -> Vec<RawPlugin> {
    root.child("build")
        .and_then(|build| build.child("plugins"))
        .map(|plugins| plugin_children(plugins, source_path))
        .unwrap_or_default()
}

fn parse_plugin_management(root: &XmlNode, source_path: &str) -> Vec<RawPlugin> {
    root.child("build")
        .and_then(|node| node.child("pluginManagement"))
        .and_then(|node| node.child("plugins"))
        .map(|plugins| plugin_children(plugins, source_path))
        .unwrap_or_default()
}

fn plugin_children(plugins: &XmlNode, source_path: &str) -> Vec<RawPlugin> {
    plugins
        .children_named("plugin")
        .filter_map(|plugin| {
            let artifact_id = child_text(plugin, "artifactId");
            artifact_id.as_ref()?;
            Some(RawPlugin {
                group_id: child_text(plugin, "groupId"),
                line: artifact_id.as_ref().map(|value| value.line).unwrap_or(1),
                artifact_id,
                version: child_text(plugin, "version"),
                inherited: child_text(plugin, "inherited"),
                executions: plugin
                    .child("executions")
                    .map(parse_plugin_executions)
                    .unwrap_or_default(),
                source_path: source_path.to_owned(),
            })
        })
        .collect()
}

fn parse_plugin_executions(executions: &XmlNode) -> Vec<RawPluginExecution> {
    executions
        .children_named("execution")
        .map(|execution| {
            let id = child_text(execution, "id");
            let phase = child_text(execution, "phase");
            RawPluginExecution {
                line: id
                    .as_ref()
                    .or(phase.as_ref())
                    .map(|value| value.line)
                    .unwrap_or(1),
                id,
                phase,
                inherited: child_text(execution, "inherited"),
                goals: execution
                    .child("goals")
                    .map(|goals| {
                        goals
                            .children_named("goal")
                            .filter_map(tagged_node)
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn child_text(node: &XmlNode, name: &str) -> Option<TaggedValue> {
    tagged_node(node.child(name)?)
}

fn child_text_allow_empty(node: &XmlNode, name: &str) -> Option<TaggedValue> {
    let child = node.child(name)?;
    let value = child.text.trim();
    Some(TaggedValue {
        value: value.to_owned(),
        line: child.line,
    })
}

fn tagged_node(node: &XmlNode) -> Option<TaggedValue> {
    let value = node.text.trim();
    (!value.is_empty()).then(|| TaggedValue {
        value: value.to_owned(),
        line: node.line,
    })
}
