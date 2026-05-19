use serde::{Deserialize, Serialize};

use super::{
    DomainError,
    code_repository::{CodeQueryKind, CodeRetrievalHit},
    error::required_text,
    retrieval::FreshnessPolicy,
};

/// Repository-set creation request shared by API adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetCreateRequest {
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub default_ref_policy_json: String,
}

impl CodeRepositorySetCreateRequest {
    pub fn new(
        alias: impl Into<String>,
        description: Option<String>,
        default_ref_policy_json: Option<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            alias: required_text("set_alias", alias)?,
            description: optional_text("description", description)?,
            default_ref_policy_json: default_ref_policy_json
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "{\"default_ref\":\"HEAD\"}".to_owned()),
        })
    }
}

/// Persisted repository-set metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySet {
    pub set_id: String,
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub default_ref_policy_json: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Request to attach one indexed repository snapshot to a repository set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetAddMemberRequest {
    pub set_alias: String,
    pub repository_alias: String,
    pub ref_selector: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub priority: i32,
}

impl CodeRepositorySetAddMemberRequest {
    pub fn new(
        set_alias: impl Into<String>,
        repository_alias: impl Into<String>,
        ref_selector: impl Into<String>,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        priority: i32,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            set_alias: required_text("set_alias", set_alias)?,
            repository_alias: required_text("repository_alias", repository_alias)?,
            ref_selector: required_text("ref_selector", ref_selector)?,
            path_filters: normalize_filter_list("path_filter", path_filters)?,
            language_filters: normalize_filter_list("language_filter", language_filters)?,
            priority,
        })
    }
}

/// Persisted repository-set membership pointing at a real repository snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetMember {
    pub set_id: String,
    pub repository_id: String,
    pub repository_alias: String,
    pub ref_selector: String,
    pub resolved_commit_sha: String,
    pub source_scope: String,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
    pub priority: i32,
}

/// Status for one repository-set member snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetMemberStatus {
    pub member: CodeRepositorySetMember,
    pub tree_hash: String,
    pub freshness_state: String,
    pub stale: bool,
    pub indexed_file_count: usize,
    pub symbol_count: usize,
    pub reference_count: usize,
    pub chunk_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Overlay freshness and task-independent diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetOverlayStatus {
    pub state: String,
    pub stale: bool,
    pub edge_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Aggregated repository-set status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetStatus {
    pub repository_set: CodeRepositorySet,
    pub members: Vec<CodeRepositorySetMemberStatus>,
    pub overlay: CodeRepositorySetOverlayStatus,
    pub freshness_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

/// Multi-repository query request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetQueryRequest {
    pub set_alias: String,
    pub query: String,
    pub code_query_kind: CodeQueryKind,
    pub limit: usize,
    pub freshness_policy: FreshnessPolicy,
    pub path_filters: Vec<String>,
    pub language_filters: Vec<String>,
}

impl CodeRepositorySetQueryRequest {
    pub fn new(
        set_alias: impl Into<String>,
        query: impl Into<String>,
        code_query_kind: CodeQueryKind,
        limit: usize,
        freshness_policy: FreshnessPolicy,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> Result<Self, DomainError> {
        let limit = match limit {
            1..=50 => limit,
            0 => return Err(DomainError::invalid("limit", "must be greater than zero")),
            _ => return Err(DomainError::invalid("limit", "must be 50 or less")),
        };

        Ok(Self {
            set_alias: required_text("set_alias", set_alias)?,
            query: required_text("query", query)?,
            code_query_kind,
            limit,
            freshness_policy,
            path_filters: normalize_filter_list("path_filter", path_filters)?,
            language_filters: normalize_filter_list("language_filter", language_filters)?,
        })
    }
}

/// Cross-repository overlay edge derived after member snapshots are indexed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryCrossEdge {
    pub edge_id: String,
    pub set_id: String,
    pub from_source_scope: String,
    pub from_repository_id: String,
    pub from_record_kind: String,
    pub from_record_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_repository_id: Option<String>,
    pub to_record_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_record_id: Option<String>,
    pub edge_kind: String,
    pub resolution_state: String,
    pub confidence_basis_points: u16,
    pub confidence_tier: String,
    pub evidence_json: String,
    pub created_at_ms: u64,
}

/// Query hit with explicit repository-set provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeRepositorySetQueryHit {
    pub member: CodeRepositorySetMember,
    pub hit: CodeRetrievalHit,
    pub overlay_evidence: Vec<CodeRepositoryCrossEdge>,
    pub score: f64,
}

/// Summary returned after rebuilding a repository-set overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetRefreshSummary {
    pub set_id: String,
    pub alias: String,
    pub edge_count: usize,
    pub resolved_edge_count: usize,
    pub ambiguous_edge_count: usize,
    pub unresolved_edge_count: usize,
    pub refreshed_at_ms: u64,
}

/// Durable state for a repository-set overlay refresh task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeRepositorySetRefreshTaskState {
    Queued,
    Running,
    Succeeded,
    Retrying,
    DeadLetter,
}

impl CodeRepositorySetRefreshTaskState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Retrying => "retrying",
            Self::DeadLetter => "dead_letter",
        }
    }

    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "retrying" => Ok(Self::Retrying),
            "dead_letter" => Ok(Self::DeadLetter),
            _ => Err(DomainError::invalid(
                "repository_set_refresh_task_state",
                "unknown repository set refresh task state",
            )),
        }
    }

    pub const fn is_unfinished(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Retrying)
    }
}

/// Durable repository-set overlay refresh task record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositorySetRefreshTaskRecord {
    pub task_id: String,
    pub set_id: String,
    pub set_alias: String,
    pub state: CodeRepositorySetRefreshTaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    pub attempt_count: u32,
    pub next_retry_at_ms: u64,
    pub input_fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_message: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

fn optional_text(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, DomainError> {
    value.map(|value| required_text(field, value)).transpose()
}

fn normalize_filter_list(
    field: &'static str,
    values: Vec<String>,
) -> Result<Vec<String>, DomainError> {
    let mut normalized = Vec::new();
    for value in values {
        let value = required_text(field, value)?;
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    Ok(normalized)
}
