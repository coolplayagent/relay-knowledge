use serde::{Deserialize, Serialize};

use super::super::{
    CodeWorkspaceDetectionConfig, DomainError, FreshnessPolicy, error::required_text,
};
use super::validation::{checked_u32, normalize_filter_list};

/// Inclusive byte or line range for repository code index rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCodeRange {
    pub start: u32,
    pub end: u32,
}

impl RepositoryCodeRange {
    /// Creates an ordered range using one-based lines or zero-based bytes.
    pub fn new(field: &'static str, start: usize, end: usize) -> Result<Self, DomainError> {
        if end < start {
            return Err(DomainError::invalid(
                field,
                "end must be greater than or equal to start",
            ));
        }

        Ok(Self {
            start: checked_u32(field, start)?,
            end: checked_u32(field, end)?,
        })
    }
}

/// Code repository identity persisted after registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryRegistration {
    pub repository_id: String,
    pub alias: String,
    pub root_path: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

impl CodeRepositoryRegistration {
    /// Validates a repository registration before storage persists it.
    pub fn new(
        repository_id: impl Into<String>,
        alias: impl Into<String>,
        root_path: impl Into<String>,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            repository_id: required_text("repository_id", repository_id)?,
            alias: required_text("alias", alias)?,
            root_path: required_text("root_path", root_path)?,
            path_filters: normalize_filter_list("path_filter", path_filters)?,
            language_filters: normalize_filter_list("language_filter", language_filters)?,
        })
    }
}

/// Repository selector accepted by code index and retrieval APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySelector {
    pub repository: String,
    pub ref_selector: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

impl CodeRepositorySelector {
    /// Validates a code repository selector with an explicit ref.
    pub fn new(
        repository: impl Into<String>,
        ref_selector: impl Into<String>,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            repository: required_text("repository", repository)?,
            ref_selector: required_text("ref_selector", ref_selector)?,
            path_filters: normalize_filter_list("path_filter", path_filters)?,
            language_filters: normalize_filter_list("language_filter", language_filters)?,
        })
    }
}

/// Code index mode tied to Git snapshots or diffs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeIndexMode {
    Full,
    Incremental { base_ref: String, head_ref: String },
    WorktreeOverlay,
}

impl CodeIndexMode {
    /// Validates incremental refs and preserves the mode contract.
    pub fn incremental(
        base_ref: impl Into<String>,
        head_ref: impl Into<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self::Incremental {
            base_ref: required_text("base_ref", base_ref)?,
            head_ref: required_text("head_ref", head_ref)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeIndexRequest {
    pub repository: CodeRepositorySelector,
    pub mode: CodeIndexMode,
    #[serde(default)]
    pub workspace_detection: CodeWorkspaceDetectionConfig,
    pub freshness_policy: FreshnessPolicy,
}
