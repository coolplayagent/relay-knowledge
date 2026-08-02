use crate::domain::{
    AuditStatus, GraphVersion, ProposalConflictSeverity, ProposalKind, ProposalProvenance,
    ProposalState, ServiceOperatorState, WorkerKind,
};

/// Worker task input inserted after graph changes or service reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskSeed {
    pub kind: WorkerKind,
    pub source_scope: String,
    pub evidence_id: Option<String>,
    pub target_graph_version: GraphVersion,
    pub input_fingerprint: String,
    pub payload_json: String,
    pub now_ms: u64,
}

/// Worker lease acquisition request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskClaimRequest {
    pub kind: Option<WorkerKind>,
    pub lease_owner: String,
    pub lease_duration_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// Worker completion guarded by the active lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskCompletion {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub now_ms: u64,
}

/// Worker failure report for retry and dead-letter handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTaskFailure {
    pub task_id: String,
    pub lease_owner: String,
    pub attempt_count: u32,
    pub error_kind: String,
    pub error_message: String,
    pub retry_backoff_ms: u64,
    pub max_attempts: u32,
    pub now_ms: u64,
}

/// New proposal to persist before manual approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProposal {
    pub proposal_id: String,
    pub source_scope: String,
    pub kind: ProposalKind,
    pub title: String,
    pub summary: String,
    pub payload_json: String,
    pub origin: String,
    pub provenance: ProposalProvenance,
    pub confidence_basis_points: u16,
    pub conflicts: Vec<NewProposalConflict>,
    pub now_ms: u64,
}

/// New proposal conflict to persist with a proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProposalConflict {
    pub conflict_id: String,
    pub existing_fact_kind: String,
    pub existing_fact_id: String,
    pub severity: ProposalConflictSeverity,
    pub reason: String,
}

/// Proposal list filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalListRequest {
    pub state: Option<ProposalState>,
    pub limit: usize,
}

/// Proposal decision request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalDecision {
    pub proposal_id: String,
    pub next_state: ProposalState,
    pub actor: String,
    pub reason: Option<String>,
    pub now_ms: u64,
}

/// New durable audit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAuditEvent {
    pub operation: String,
    pub interface: String,
    pub request_id: String,
    pub trace_id: String,
    pub status: AuditStatus,
    pub actor: Option<String>,
    pub source_scope: Option<String>,
    pub graph_version: u64,
    pub detail_json: String,
    pub message: Option<String>,
    pub now_ms: u64,
}

/// Audit query filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditQueryRequest {
    pub operation: Option<String>,
    pub limit: usize,
}

/// Service operator state update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceOperatorUpdate {
    pub state: ServiceOperatorState,
    pub silent_updates_enabled: bool,
    pub allowed_scopes: Vec<String>,
    pub last_error: Option<String>,
    pub now_ms: u64,
}
