use serde::{Deserialize, Serialize};

use crate::domain::{
    CodeIndexCheckpoint, CodeIndexTaskQueueStatus, CodeIndexTaskRecord, FreshnessPolicy,
};

/// Freshness state for a code repository graph answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeRepositoryFreshnessState {
    Fresh,
    Pending,
    Stale,
    Degraded,
}

/// Durable code-index cursor/checkpoint surfaced with graph answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryFreshnessCursor {
    pub source_scope: String,
    pub checkpoint_state: String,
    pub total_path_count: usize,
    pub parsed_file_count: usize,
    pub committed_file_count: usize,
    pub committed_symbol_count: usize,
    pub committed_reference_count: usize,
    pub committed_chunk_count: usize,
    pub batch_count: usize,
    pub pending_file_count: usize,
    pub updated_at_ms: u64,
}

impl CodeRepositoryFreshnessCursor {
    pub fn from_checkpoint(checkpoint: &CodeIndexCheckpoint) -> Self {
        Self {
            source_scope: checkpoint.source_scope.clone(),
            checkpoint_state: checkpoint.state.clone(),
            total_path_count: checkpoint.total_path_count,
            parsed_file_count: checkpoint.parsed_file_count,
            committed_file_count: checkpoint.committed_file_count,
            committed_symbol_count: checkpoint.committed_symbol_count,
            committed_reference_count: checkpoint.committed_reference_count,
            committed_chunk_count: checkpoint.committed_chunk_count,
            batch_count: checkpoint.batch_count,
            pending_file_count: checkpoint
                .total_path_count
                .saturating_sub(checkpoint.committed_file_count),
            updated_at_ms: checkpoint.updated_at_ms,
        }
    }
}

/// Pending code-index work that can make a graph answer stale.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryPendingIndexWork {
    pub active_for_repository: bool,
    pub active_matches_request: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task_source_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task_ref_selector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task_resolved_commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_task_lease_expires_at_ms: Option<u64>,
    pub queue_depth: usize,
    pub queued_task_count: usize,
    pub running_task_count: usize,
    pub retrying_task_count: usize,
    pub dead_letter_task_count: usize,
    pub running_lease_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl CodeRepositoryPendingIndexWork {
    pub fn from_task_and_queue(
        task: Option<&CodeIndexTaskRecord>,
        active_matches_request: bool,
        queue: CodeIndexTaskQueueStatus,
    ) -> Self {
        let queue_depth = queue
            .queued_task_count
            .saturating_add(queue.running_task_count)
            .saturating_add(queue.retrying_task_count);

        Self {
            active_for_repository: task.is_some(),
            active_matches_request,
            active_task_id: task.map(|task| task.task_id.clone()),
            active_task_state: task.map(|task| task.state.as_str().to_owned()),
            active_task_source_scope: task.map(|task| task.source_scope.clone()),
            active_task_ref_selector: task.map(|task| task.ref_selector.clone()),
            active_task_resolved_commit_sha: task.map(|task| task.resolved_commit_sha.clone()),
            active_task_lease_expires_at_ms: task.and_then(|task| task.lease_expires_at_ms),
            queue_depth,
            queued_task_count: queue.queued_task_count,
            running_task_count: queue.running_task_count,
            retrying_task_count: queue.retrying_task_count,
            dead_letter_task_count: queue.dead_letter_task_count,
            running_lease_count: queue.running_lease_count,
            last_error: queue.last_error,
        }
    }
}

/// Ref and file-count lag between requested source and served graph state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryIndexLag {
    pub requested_ref: String,
    pub requested_resolved_ref: String,
    pub served_ref: String,
    pub requested_ref_indexed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_file_count: Option<usize>,
    pub pending_task_count: usize,
}

/// Freshness governance fields returned with code graph answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeRepositoryFreshnessDiagnostics {
    pub state: CodeRepositoryFreshnessState,
    pub freshness_policy: FreshnessPolicy,
    pub graph_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<String>,
    pub scope_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub index_lag: CodeRepositoryIndexLag,
    pub pending: CodeRepositoryPendingIndexWork,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<CodeRepositoryFreshnessCursor>,
    pub direct_source_read_required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct_source_read_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_instructions: Vec<String>,
}

impl CodeRepositoryFreshnessDiagnostics {
    pub fn legacy_unknown() -> Self {
        Self {
            state: CodeRepositoryFreshnessState::Degraded,
            freshness_policy: FreshnessPolicy::AllowStale,
            graph_version: 0,
            source_scope: None,
            scope_stale: false,
            stale_reason: None,
            degraded_reason: Some(
                "remote response did not include freshness diagnostics".to_owned(),
            ),
            index_lag: CodeRepositoryIndexLag {
                requested_ref: String::new(),
                requested_resolved_ref: String::new(),
                served_ref: String::new(),
                requested_ref_indexed: false,
                pending_file_count: None,
                pending_task_count: 0,
            },
            pending: CodeRepositoryPendingIndexWork::default(),
            cursor: None,
            direct_source_read_required: false,
            direct_source_read_paths: Vec::new(),
            agent_instructions: Vec::new(),
        }
    }

    pub(crate) fn code_query(input: CodeRepositoryFreshnessInput) -> Self {
        let requested_ref_indexed =
            !input.scope_stale && input.requested_resolved_ref == input.served_ref;
        let pending_file_count = input
            .cursor
            .as_ref()
            .map(|cursor| cursor.pending_file_count);
        let pending_task_count = input.pending.queue_depth;
        let direct_source_read_required = !requested_ref_indexed || input.scope_stale;
        let state = freshness_state(
            direct_source_read_required,
            input.pending.active_matches_request,
            input.scope_stale,
            input.degraded_reason.as_ref(),
        );
        let agent_instructions = source_read_instructions(
            direct_source_read_required,
            &input.requested_ref,
            &input.served_ref,
            &input.direct_source_read_paths,
        );

        Self {
            state,
            freshness_policy: input.freshness_policy,
            graph_version: input.graph_version,
            source_scope: input.source_scope,
            scope_stale: input.scope_stale,
            stale_reason: input.stale_reason,
            degraded_reason: input.degraded_reason,
            index_lag: CodeRepositoryIndexLag {
                requested_ref: input.requested_ref,
                requested_resolved_ref: input.requested_resolved_ref,
                served_ref: input.served_ref,
                requested_ref_indexed,
                pending_file_count,
                pending_task_count,
            },
            pending: input.pending,
            cursor: input.cursor,
            direct_source_read_required,
            direct_source_read_paths: input.direct_source_read_paths,
            agent_instructions,
        }
    }

    pub(crate) fn graph_only(
        graph_version: u64,
        freshness_policy: FreshnessPolicy,
        source_scope: Option<String>,
        requested_ref: String,
        degraded_reason: String,
    ) -> Self {
        let input = CodeRepositoryFreshnessInput {
            graph_version,
            freshness_policy,
            source_scope,
            requested_ref: requested_ref.clone(),
            requested_resolved_ref: requested_ref.clone(),
            served_ref: requested_ref,
            scope_stale: false,
            stale_reason: None,
            degraded_reason: Some(degraded_reason),
            pending: CodeRepositoryPendingIndexWork::default(),
            cursor: None,
            direct_source_read_paths: Vec::new(),
        };

        Self::code_query(input)
    }
}

pub(crate) struct CodeRepositoryFreshnessInput {
    pub graph_version: u64,
    pub freshness_policy: FreshnessPolicy,
    pub source_scope: Option<String>,
    pub requested_ref: String,
    pub requested_resolved_ref: String,
    pub served_ref: String,
    pub scope_stale: bool,
    pub stale_reason: Option<String>,
    pub degraded_reason: Option<String>,
    pub pending: CodeRepositoryPendingIndexWork,
    pub cursor: Option<CodeRepositoryFreshnessCursor>,
    pub direct_source_read_paths: Vec<String>,
}

fn freshness_state(
    direct_source_read_required: bool,
    active_matches_request: bool,
    scope_stale: bool,
    degraded_reason: Option<&String>,
) -> CodeRepositoryFreshnessState {
    if direct_source_read_required && active_matches_request {
        CodeRepositoryFreshnessState::Pending
    } else if scope_stale || direct_source_read_required {
        CodeRepositoryFreshnessState::Stale
    } else if degraded_reason.is_some() {
        CodeRepositoryFreshnessState::Degraded
    } else {
        CodeRepositoryFreshnessState::Fresh
    }
}

fn source_read_instructions(
    required: bool,
    requested_ref: &str,
    served_ref: &str,
    paths: &[String],
) -> Vec<String> {
    if !required {
        return Vec::new();
    }
    let mut instructions = vec![format!(
        "Code graph results were served from indexed ref {served_ref}; read direct source before relying on files changed at requested ref {requested_ref}."
    )];
    if !paths.is_empty() {
        instructions.push(format!(
            "Verify returned paths from direct source before editing or citing them: {}.",
            paths.join(", ")
        ));
    }

    instructions
}
