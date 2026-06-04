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
    Uninstall,
}

impl ServiceManagerAction {
    /// Stable CLI and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
        }
    }

    /// Parses the stable CLI and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "install" => Ok(Self::Install),
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
    pub platform: String,
    pub service_name: String,
    pub definition_path: String,
    pub definition: String,
    pub install_command: Vec<String>,
    pub uninstall_command: Vec<String>,
    pub start_command: Vec<String>,
    pub stop_command: Vec<String>,
    pub runtime_state_paths: Vec<String>,
    pub warnings: Vec<String>,
    pub checksum: String,
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
mod tests {
    use super::*;

    #[test]
    fn operational_enums_have_stable_storage_values() {
        for (kind, value) in [
            (WorkerKind::Embedding, "embedding"),
            (WorkerKind::Ocr, "ocr"),
            (WorkerKind::Vision, "vision"),
            (WorkerKind::Extractor, "extractor"),
        ] {
            assert_eq!(kind.as_str(), value);
            assert_eq!(WorkerKind::parse(value).expect("worker kind"), kind);
        }
        for (state, value) in [
            (WorkerTaskState::Queued, "queued"),
            (WorkerTaskState::Running, "running"),
            (WorkerTaskState::Succeeded, "succeeded"),
            (WorkerTaskState::Retrying, "retrying"),
            (WorkerTaskState::Failed, "failed"),
            (WorkerTaskState::DeadLetter, "dead_letter"),
        ] {
            assert_eq!(state.as_str(), value);
            assert_eq!(WorkerTaskState::parse(value).expect("task state"), state);
        }
        for (state, value) in [
            (WorkerBackendState::Fallback, "fallback"),
            (WorkerBackendState::Configured, "configured"),
            (WorkerBackendState::Degraded, "degraded"),
            (WorkerBackendState::Unavailable, "unavailable"),
        ] {
            assert_eq!(state.as_str(), value);
        }
    }

    #[test]
    fn proposal_audit_and_operator_enums_have_stable_values() {
        for (kind, value) in [
            (ProposalKind::Evidence, "evidence"),
            (ProposalKind::Relation, "relation"),
            (ProposalKind::Claim, "claim"),
            (ProposalKind::Event, "event"),
        ] {
            assert_eq!(kind.as_str(), value);
            assert_eq!(ProposalKind::parse(value).expect("proposal kind"), kind);
        }
        for (state, value) in [
            (ProposalState::Proposed, "proposed"),
            (ProposalState::Accepted, "accepted"),
            (ProposalState::Rejected, "rejected"),
            (ProposalState::Superseded, "superseded"),
        ] {
            assert_eq!(state.as_str(), value);
            assert_eq!(ProposalState::parse(value).expect("proposal state"), state);
        }
        for (severity, value) in [
            (ProposalConflictSeverity::Info, "info"),
            (ProposalConflictSeverity::Warning, "warning"),
            (ProposalConflictSeverity::Blocking, "blocking"),
        ] {
            assert_eq!(severity.as_str(), value);
            assert_eq!(
                ProposalConflictSeverity::parse(value).expect("conflict severity"),
                severity
            );
        }
        for (status, value) in [
            (AuditStatus::Started, "started"),
            (AuditStatus::Completed, "completed"),
            (AuditStatus::Failed, "failed"),
            (AuditStatus::Cancelled, "cancelled"),
        ] {
            assert_eq!(status.as_str(), value);
            assert_eq!(AuditStatus::parse(value).expect("audit status"), status);
        }
        for (state, value) in [
            (ServiceOperatorState::Disabled, "disabled"),
            (ServiceOperatorState::Enabled, "enabled"),
            (ServiceOperatorState::Paused, "paused"),
            (ServiceOperatorState::Degraded, "degraded"),
            (ServiceOperatorState::Failed, "failed"),
        ] {
            assert_eq!(state.as_str(), value);
            assert_eq!(
                ServiceOperatorState::parse(value).expect("operator state"),
                state
            );
        }
        for (action, value) in [
            (ServiceManagerAction::Install, "install"),
            (ServiceManagerAction::Uninstall, "uninstall"),
        ] {
            assert_eq!(action.as_str(), value);
            assert_eq!(
                ServiceManagerAction::parse(value).expect("service action"),
                action
            );
        }
    }

    #[test]
    fn invalid_operational_values_are_rejected_or_redacted() {
        assert!(WorkerKind::parse("gpu").is_err());
        assert!(ProposalState::parse("merged").is_err());
        assert!(AuditStatus::parse("pending").is_err());
        assert!(ServiceManagerAction::parse("restart").is_err());
        assert!(normalize_actor("  ").is_err());

        let proposal = ProposalRecord {
            proposal_id: "proposal:test".to_owned(),
            source_scope: "docs".to_owned(),
            kind: ProposalKind::Evidence,
            state: ProposalState::Proposed,
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            payload_json: "{".to_owned(),
            origin: "test".to_owned(),
            provenance: ProposalProvenance::new("test"),
            confidence_basis_points: 1,
            conflict_count: 0,
            decided_by: None,
            decision_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        };

        assert!(proposal.payload_value().is_null());
    }

    #[test]
    fn proposal_provenance_normalizes_and_validates_lineage() {
        let provenance = ProposalProvenance {
            producer: " llm_spo_extraction ".to_owned(),
            provider: Some(" openai-compatible ".to_owned()),
            model: Some(" graph-extractor ".to_owned()),
            prompt_id: Some(" relay.extract.spo ".to_owned()),
            prompt_version: Some(" 1 ".to_owned()),
            schema_version: Some(" worker-proposal.v2 ".to_owned()),
            input_source_hash: Some(" sha256:source ".to_owned()),
            input_fact_ids: vec![" ev-1 ".to_owned(), "ev-1".to_owned()],
            stale_when: vec![" graph_version_advances ".to_owned()],
            budget_notes: vec![" timeout_ms=30000 ".to_owned()],
        }
        .validate()
        .expect("provenance should validate");

        assert_eq!(provenance.producer, "llm_spo_extraction");
        assert_eq!(provenance.input_fact_ids, ["ev-1"]);
        assert_eq!(
            ProposalProvenance::from_json(&provenance.to_json())
                .expect("stored provenance should parse"),
            provenance
        );
        assert_eq!(
            ProposalProvenance::from_json("{}")
                .expect("legacy provenance should default")
                .producer,
            "unspecified"
        );
        assert_eq!(
            ProposalProvenance::new(" ")
                .validate()
                .expect_err("empty producer should fail")
                .field,
            "proposal_producer"
        );
    }
}
