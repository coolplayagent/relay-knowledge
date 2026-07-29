use serde::{Deserialize, Serialize};

use super::{DomainError, GraphVersion, error::required_text};

/// External or fallback worker family used by background productization tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    Embedding,
    Ocr,
    Vision,
    Extractor,
}

impl WorkerKind {
    pub const ALL: [Self; 4] = [Self::Embedding, Self::Ocr, Self::Vision, Self::Extractor];

    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedding => "embedding",
            Self::Ocr => "ocr",
            Self::Vision => "vision",
            Self::Extractor => "extractor",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "embedding" => Ok(Self::Embedding),
            "ocr" => Ok(Self::Ocr),
            "vision" => Ok(Self::Vision),
            "extractor" => Ok(Self::Extractor),
            _ => Err(DomainError::invalid("worker_kind", "unknown worker kind")),
        }
    }
}

/// Persistent task lifecycle for bounded worker queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerTaskState {
    Queued,
    Running,
    Succeeded,
    Retrying,
    Failed,
    DeadLetter,
}

impl WorkerTaskState {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Retrying => "retrying",
            Self::Failed => "failed",
            Self::DeadLetter => "dead_letter",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "retrying" => Ok(Self::Retrying),
            "failed" => Ok(Self::Failed),
            "dead_letter" => Ok(Self::DeadLetter),
            _ => Err(DomainError::invalid(
                "worker_task_state",
                "unknown worker task state",
            )),
        }
    }
}

/// Runtime availability of an external worker backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerBackendState {
    Fallback,
    Configured,
    Degraded,
    Unavailable,
}

impl WorkerBackendState {
    /// Stable API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fallback => "fallback",
            Self::Configured => "configured",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Persistent worker task used by service, CLI, Web, and recovery diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerTaskRecord {
    pub task_id: String,
    pub kind: WorkerKind,
    pub source_scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    pub target_graph_version: GraphVersion,
    pub state: WorkerTaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    pub attempt_count: u32,
    pub next_retry_at_ms: u64,
    pub input_fingerprint: String,
    pub payload_json: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_message: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Aggregated status for a worker family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub kind: WorkerKind,
    pub backend_state: WorkerBackendState,
    pub endpoint_configured: bool,
    pub queue_depth: usize,
    pub running_count: usize,
    pub retrying_count: usize,
    pub dead_letter_count: usize,
    pub last_error: Option<String>,
}

/// Proposal fact family stored before user approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Evidence,
    Relation,
    Claim,
    Event,
}

impl ProposalKind {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Evidence => "evidence",
            Self::Relation => "relation",
            Self::Claim => "claim",
            Self::Event => "event",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "evidence" => Ok(Self::Evidence),
            "relation" => Ok(Self::Relation),
            "claim" => Ok(Self::Claim),
            "event" => Ok(Self::Event),
            _ => Err(DomainError::invalid(
                "proposal_kind",
                "unknown proposal kind",
            )),
        }
    }
}

/// Proposal approval lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalState {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
}

impl ProposalState {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "superseded" => Ok(Self::Superseded),
            _ => Err(DomainError::invalid(
                "proposal_state",
                "unknown proposal state",
            )),
        }
    }
}

/// Conflict severity shown before manual approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalConflictSeverity {
    Info,
    Warning,
    Blocking,
}

impl ProposalConflictSeverity {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Blocking => "blocking",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "blocking" => Ok(Self::Blocking),
            _ => Err(DomainError::invalid(
                "proposal_conflict_severity",
                "unknown proposal conflict severity",
            )),
        }
    }
}

/// Stored proposal ready for CLI/Web review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalRecord {
    pub proposal_id: String,
    pub source_scope: String,
    pub kind: ProposalKind,
    pub state: ProposalState,
    pub title: String,
    pub summary: String,
    pub payload_json: String,
    pub origin: String,
    pub provenance: ProposalProvenance,
    pub confidence_basis_points: u16,
    pub conflict_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl ProposalRecord {
    /// Returns proposal payload as JSON for API consumers that need typed preview.
    pub fn payload_value(&self) -> serde_json::Value {
        serde_json::from_str(&self.payload_json).unwrap_or(serde_json::Value::Null)
    }
}

/// Auditable model, prompt, and source lineage for a stored proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalProvenance {
    pub producer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_fact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_when: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub budget_notes: Vec<String>,
}

impl Default for ProposalProvenance {
    fn default() -> Self {
        Self::new("unspecified")
    }
}

impl ProposalProvenance {
    /// Creates a minimal provenance record for deterministic or manual proposal producers.
    pub fn new(producer: impl Into<String>) -> Self {
        Self {
            producer: producer.into(),
            provider: None,
            model: None,
            prompt_id: None,
            prompt_version: None,
            schema_version: None,
            input_source_hash: None,
            input_fact_ids: Vec::new(),
            stale_when: Vec::new(),
            budget_notes: Vec::new(),
        }
    }

    /// Parses stored JSON while preserving legacy rows that predate provenance metadata.
    pub fn from_json(value: &str) -> Result<Self, DomainError> {
        if value.trim().is_empty() || value.trim() == "{}" {
            return Ok(Self::default());
        }

        serde_json::from_str::<Self>(value)
            .map_err(|_| DomainError::invalid("proposal_provenance", "must be valid JSON"))?
            .validate()
    }

    /// Serializes provenance metadata for storage.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Normalizes and validates stable provenance fields.
    pub fn validate(mut self) -> Result<Self, DomainError> {
        self.producer = required_text("proposal_producer", self.producer)?;
        self.provider = normalize_optional_text("proposal_provider", self.provider)?;
        self.model = normalize_optional_text("proposal_model", self.model)?;
        self.prompt_id = normalize_optional_text("proposal_prompt_id", self.prompt_id)?;
        self.prompt_version =
            normalize_optional_text("proposal_prompt_version", self.prompt_version)?;
        self.schema_version =
            normalize_optional_text("proposal_schema_version", self.schema_version)?;
        self.input_source_hash =
            normalize_optional_text("proposal_input_source_hash", self.input_source_hash)?;
        self.input_fact_ids = normalize_text_list("proposal_input_fact_id", self.input_fact_ids)?;
        self.stale_when = normalize_text_list("proposal_stale_condition", self.stale_when)?;
        self.budget_notes = normalize_text_list("proposal_budget_note", self.budget_notes)?;

        Ok(self)
    }
}

/// Stored conflict associated with a proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalConflictRecord {
    pub conflict_id: String,
    pub proposal_id: String,
    pub existing_fact_kind: String,
    pub existing_fact_id: String,
    pub severity: ProposalConflictSeverity,
    pub reason: String,
}

/// Persistent audit status shared by CLI/Web/service/agent surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditStatus {
    Started,
    Completed,
    Failed,
    Cancelled,
}

impl AuditStatus {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "started" => Ok(Self::Started),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(DomainError::invalid("audit_status", "unknown audit status")),
        }
    }
}

/// Redacted durable audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEventRecord {
    pub sequence: u64,
    pub operation: String,
    pub interface: String,
    pub request_id: String,
    pub trace_id: String,
    pub status: AuditStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub graph_version: u64,
    pub detail_json: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub created_at_ms: u64,
}

/// Installed background operator state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceOperatorState {
    Disabled,
    Enabled,
    Paused,
    Degraded,
    Failed,
}

impl ServiceOperatorState {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
            Self::Paused => "paused",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "disabled" => Ok(Self::Disabled),
            "enabled" => Ok(Self::Enabled),
            "paused" => Ok(Self::Paused),
            "degraded" => Ok(Self::Degraded),
            "failed" => Ok(Self::Failed),
            _ => Err(DomainError::invalid(
                "service_operator_state",
                "unknown service operator state",
            )),
        }
    }
}

/// Persisted silent-update operator status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceOperatorStatus {
    pub state: ServiceOperatorState,
    pub silent_updates_enabled: bool,
    pub allowed_scopes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_retry_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub updated_at_ms: u64,
}

/// Service manager action surfaced as a generated, user-executed plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceManagerAction {
    Install,
    Upgrade,
    Rollback,
    Uninstall,
}

impl ServiceManagerAction {
    /// Stable CLI and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Upgrade => "upgrade",
            Self::Rollback => "rollback",
            Self::Uninstall => "uninstall",
        }
    }

    /// Parses the stable CLI and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "install" => Ok(Self::Install),
            "upgrade" => Ok(Self::Upgrade),
            "rollback" => Ok(Self::Rollback),
            "uninstall" => Ok(Self::Uninstall),
            _ => Err(DomainError::invalid(
                "service_manager_action",
                "unknown service manager action",
            )),
        }
    }
}

/// Service definition rendering without privileged execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDefinitionPlan {
    pub action: ServiceManagerAction,
    pub dry_run: bool,
    pub platform: String,
    pub service_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_dir: Option<String>,
    pub binary_path: String,
    pub definition_path: String,
    pub definition: String,
    pub install_command: Vec<String>,
    pub uninstall_command: Vec<String>,
    pub start_command: Vec<String>,
    pub stop_command: Vec<String>,
    pub lifecycle_steps: Vec<ServiceLifecycleStep>,
    pub rollback_steps: Vec<ServiceLifecycleStep>,
    pub permission_requirements: Vec<ServicePermissionRequirement>,
    pub package_manifest_checks: Vec<ServicePackageManifestCheck>,
    pub runtime_state_paths: Vec<String>,
    pub checkpoint_path: String,
    pub warnings: Vec<String>,
    pub checksum: String,
}

/// One staged service lifecycle action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLifecycleStep {
    pub id: String,
    pub phase: String,
    pub description: String,
    pub command: Vec<String>,
    pub writes_paths: Vec<String>,
    pub removes_paths: Vec<String>,
    pub requires_privilege: bool,
}

/// Permission requirement attached to a generated service lifecycle plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePermissionRequirement {
    pub scope: String,
    pub reason: String,
}

/// Package-manager release-manifest drift check described by the lifecycle plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePackageManifestCheck {
    pub manager: String,
    pub artifact_source: String,
    pub verification: String,
}

/// Execution result for a lifecycle step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLifecycleStepResult {
    pub step_id: String,
    pub status: String,
    pub message: String,
}

/// Result of executing or dry-running a service lifecycle plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLifecycleExecutionReport {
    pub executed: bool,
    pub dry_run: bool,
    pub completed_steps: Vec<ServiceLifecycleStepResult>,
    pub rollback_steps: Vec<ServiceLifecycleStepResult>,
    pub rolled_back: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_step_id: Option<String>,
}

/// Normalizes a required actor identifier for lifecycle decisions.
pub fn normalize_actor(value: impl Into<String>) -> Result<String, DomainError> {
    required_text("actor", value)
}

fn normalize_optional_text(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, DomainError> {
    value.map(|inner| required_text(field, inner)).transpose()
}

fn normalize_text_list(
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

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
