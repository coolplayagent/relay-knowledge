//! Defines dependency declarations extracted from repository manifests and lock files.

use serde::{Deserialize, Serialize};

use super::RepositoryCodeRange;

#[cfg(test)]
mod mod_tests;

/// Dependency declaration extracted from package manager manifest or lock files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeDependencyRecord {
    pub repository_id: String,
    pub source_scope: String,
    pub dependency_id: String,
    pub file_id: String,
    pub path: String,
    pub language_id: String,
    pub ecosystem: String,
    pub package_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_version: Option<String>,
    pub dependency_group: String,
    pub source_kind: String,
    pub is_lockfile: bool,
    pub line_range: RepositoryCodeRange,
    pub excerpt: String,
}
