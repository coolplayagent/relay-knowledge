use std::{collections::HashSet, path::Path};

use serde::{Deserialize, Serialize};

use super::{DomainError, error::required_text};

/// Repository navigation map selected by `relay-knowledge map --type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryMapType {
    Knowledge,
    Codespec,
}

impl RepositoryMapType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::Codespec => "codespec",
        }
    }

    pub fn required_directories(self) -> &'static [&'static str] {
        match self {
            Self::Knowledge => &["domain", "guides", "ops", "glossary", "best-practices"],
            Self::Codespec => &["requirements", "design", "api", "test", "decisions"],
        }
    }
}

/// When an agent should load content described by a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryLoadHint {
    Always,
    TaskMatch,
    OnDemand,
}

/// How authored content in a governed directory is updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryUpdateRule {
    Reviewed,
    Generated,
    ExternalSync,
}

/// Supported typed relationship between governed directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryRelationKind {
    DependsOn,
    Implements,
    Documents,
    Tests,
    Operates,
    RelatedTo,
}

/// Qualified relation target, such as `codespec:design`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryRelation {
    pub kind: DirectoryRelationKind,
    pub target: String,
}

/// Strongly typed directory governance entry persisted in a repository map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryMapDirectory {
    pub directory: String,
    pub purpose: String,
    #[serde(default)]
    pub content_scope: Vec<String>,
    #[serde(default)]
    pub key_files: Vec<String>,
    pub load_hint: DirectoryLoadHint,
    #[serde(default)]
    pub relations: Vec<DirectoryRelation>,
    pub update_rule: DirectoryUpdateRule,
}

impl RepositoryMapDirectory {
    pub fn validate(&self, map_type: RepositoryMapType) -> Result<(), DomainError> {
        validate_relative_path("directory", &self.directory)?;
        required_text("purpose", self.purpose.as_str())?;
        if self.content_scope.is_empty() {
            return Err(DomainError::invalid("content_scope", "must not be empty"));
        }
        unique_values("content_scope", &self.content_scope)?;
        unique_values("key_files", &self.key_files)?;
        for pattern in &self.content_scope {
            validate_glob(pattern)?;
            if !is_owned_path(map_type, &self.directory, pattern) {
                return Err(DomainError::invalid(
                    "content_scope",
                    "patterns must stay inside the governed directory",
                ));
            }
        }
        for path in &self.key_files {
            validate_relative_path("key_files", path)?;
            if !is_owned_path(map_type, &self.directory, path) {
                return Err(DomainError::invalid(
                    "key_files",
                    "paths must stay inside the governed directory",
                ));
            }
        }
        let mut relations = HashSet::new();
        for relation in &self.relations {
            validate_relation_target(&relation.target)?;
            if !relations.insert((relation.kind, relation.target.to_lowercase())) {
                return Err(DomainError::invalid("relations", "entries must be unique"));
            }
            let own_target = format!("{}:{}", map_type.as_str(), self.directory);
            if relation.target.eq_ignore_ascii_case(&own_target) {
                return Err(DomainError::invalid(
                    "relations",
                    "self references are not allowed",
                ));
            }
        }
        Ok(())
    }
}

/// Optional fields accepted by `map directory update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryMapDirectoryChange {
    pub directory: String,
    pub purpose: Option<String>,
    pub content_scope: Option<Vec<String>>,
    pub key_files: Option<Vec<String>>,
    pub load_hint: Option<DirectoryLoadHint>,
    pub relations: Option<Vec<DirectoryRelation>>,
    pub update_rule: Option<DirectoryUpdateRule>,
}

pub(crate) fn validate_directory_collection(
    map_type: RepositoryMapType,
    directories: &[RepositoryMapDirectory],
    require_baseline: bool,
) -> Result<(), DomainError> {
    let mut names = HashSet::new();
    for directory in directories {
        directory.validate(map_type)?;
        if !names.insert(directory.directory.to_lowercase()) {
            return Err(DomainError::invalid(
                "directories",
                "directory paths must be unique without case collisions",
            ));
        }
    }
    if require_baseline {
        for required in map_type.required_directories() {
            if !names.contains(*required) {
                return Err(DomainError::invalid(
                    "directories",
                    format!("required directory '{required}' is missing"),
                ));
            }
        }
    }
    validate_relation_graph(map_type, directories)
}

fn validate_relation_graph(
    map_type: RepositoryMapType,
    directories: &[RepositoryMapDirectory],
) -> Result<(), DomainError> {
    let local = directories
        .iter()
        .map(|entry| entry.directory.as_str())
        .collect::<HashSet<_>>();
    for entry in directories {
        for relation in &entry.relations {
            let Some((target_type, target_directory)) = relation.target.split_once(':') else {
                continue;
            };
            if target_type == map_type.as_str() && !local.contains(target_directory) {
                return Err(DomainError::invalid(
                    "relations",
                    format!("target '{}' does not exist", relation.target),
                ));
            }
        }
    }
    for entry in directories {
        let mut visiting = HashSet::new();
        if depends_on_cycle(
            map_type,
            entry.directory.as_str(),
            directories,
            &mut visiting,
        ) {
            return Err(DomainError::invalid(
                "relations",
                "depends_on relationships must be acyclic",
            ));
        }
    }
    Ok(())
}

fn depends_on_cycle<'a>(
    map_type: RepositoryMapType,
    directory: &'a str,
    directories: &'a [RepositoryMapDirectory],
    visiting: &mut HashSet<&'a str>,
) -> bool {
    if !visiting.insert(directory) {
        return true;
    }
    let prefix = format!("{}:", map_type.as_str());
    if let Some(entry) = directories
        .iter()
        .find(|entry| entry.directory == directory)
    {
        for target in entry
            .relations
            .iter()
            .filter(|relation| relation.kind == DirectoryRelationKind::DependsOn)
            .filter_map(|relation| relation.target.strip_prefix(&prefix))
        {
            if depends_on_cycle(map_type, target, directories, visiting) {
                return true;
            }
        }
    }
    visiting.remove(directory);
    false
}

fn validate_relative_path(field: &'static str, value: &str) -> Result<(), DomainError> {
    let text = required_text(field, value)?;
    let path = Path::new(text.as_str());
    if path.is_absolute()
        || value.contains('\\')
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(DomainError::invalid(
            field,
            "must be a confined POSIX relative path",
        ));
    }
    Ok(())
}

fn validate_glob(value: &str) -> Result<(), DomainError> {
    validate_relative_path("content_scope", value)?;
    let mut depth = 0_i32;
    for character in value.chars() {
        match character {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return Err(DomainError::invalid(
                "content_scope",
                "glob brackets are invalid",
            ));
        }
    }
    if depth != 0 {
        return Err(DomainError::invalid(
            "content_scope",
            "glob brackets are invalid",
        ));
    }
    Ok(())
}

fn validate_relation_target(value: &str) -> Result<(), DomainError> {
    let Some((map_type, directory)) = value.split_once(':') else {
        return Err(DomainError::invalid(
            "relations",
            "targets must use <map_type>:<directory>",
        ));
    };
    if !matches!(map_type, "knowledge" | "codespec") {
        return Err(DomainError::invalid(
            "relations",
            "target map type is invalid",
        ));
    }
    validate_relative_path("relations", directory)
}

fn unique_values(field: &'static str, values: &[String]) -> Result<(), DomainError> {
    let mut unique = HashSet::new();
    for value in values {
        required_text(field, value.as_str())?;
        if !unique.insert(value.to_lowercase()) {
            return Err(DomainError::invalid(field, "values must be unique"));
        }
    }
    Ok(())
}

fn is_owned_path(map_type: RepositoryMapType, directory: &str, value: &str) -> bool {
    let prefix = format!("{}/{directory}", map_type.as_str());
    value == prefix || value.starts_with(&format!("{prefix}/"))
}

#[cfg(test)]
#[path = "map_directory_tests.rs"]
mod tests;
