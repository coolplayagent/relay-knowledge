use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    api::{ApiError, ApiMetadata, RequestContext},
    domain::{GraphVersion, IndexKind, IndexStatus},
    indexing::IndexRefreshPlan,
    retrieval::ReadModelBackendConfig,
    storage::{
        DEFAULT_INDEX_SOURCE_SCOPE, IndexCursor, IndexRefreshClaimRequest, IndexRefreshCompletion,
        IndexRefreshDiagnostics, IndexRefreshFailure, IndexRefreshQueueRequest, IndexRefreshTask,
        IndexStore, KnowledgeStore, MutationLogEntry, StorageError,
    },
};

const INITIAL_QUEUE_DEPTH: usize = 128;
const MAX_QUEUE_DEPTH: usize = 65_536;
const LEASE_DURATION_MS: u64 = 30_000;
const RETRY_BACKOFF_MS: u64 = 250;
const MAX_ATTEMPTS: u32 = 3;
const MUTATION_LOG_PAGE_SIZE: usize = 128;

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueueCapacityPolicy {
    Required,
    DiagnosticsOnly,
}

#[derive(Clone, Copy)]
struct QueuePolicy {
    capacity: QueueCapacityPolicy,
    reset_dead_letter_tasks: bool,
}

const EXPLICIT_REFRESH_QUEUE: QueuePolicy = QueuePolicy {
    capacity: QueueCapacityPolicy::Required,
    reset_dead_letter_tasks: true,
};
const RECOVERY_REFRESH_QUEUE: QueuePolicy = QueuePolicy {
    capacity: QueueCapacityPolicy::Required,
    reset_dead_letter_tasks: false,
};
const DIAGNOSTIC_RECONCILE_QUEUE: QueuePolicy = QueuePolicy {
    capacity: QueueCapacityPolicy::DiagnosticsOnly,
    reset_dead_letter_tasks: false,
};

pub(super) struct IndexRefreshOutcome {
    pub indexes: Vec<IndexStatus>,
    pub cursors: Vec<IndexCursor>,
    pub diagnostics: IndexRefreshDiagnostics,
}

pub(super) async fn refresh_index_kinds(
    store: &Arc<dyn KnowledgeStore>,
    kinds: impl IntoIterator<Item = IndexKind>,
    graph_version: GraphVersion,
    read_models: &ReadModelBackendConfig,
) -> Result<IndexRefreshOutcome, ApiError> {
    refresh_index_kinds_with_policy(
        store,
        kinds,
        graph_version,
        EXPLICIT_REFRESH_QUEUE,
        read_models,
    )
    .await
}

pub(super) async fn recover_index_kinds(
    store: &Arc<dyn KnowledgeStore>,
    kinds: impl IntoIterator<Item = IndexKind>,
    graph_version: GraphVersion,
    read_models: &ReadModelBackendConfig,
) -> Result<IndexRefreshOutcome, ApiError> {
    refresh_index_kinds_with_policy(
        store,
        kinds,
        graph_version,
        RECOVERY_REFRESH_QUEUE,
        read_models,
    )
    .await
}

async fn refresh_index_kinds_with_policy(
    store: &Arc<dyn KnowledgeStore>,
    kinds: impl IntoIterator<Item = IndexKind>,
    graph_version: GraphVersion,
    queue_policy: QueuePolicy,
    read_models: &ReadModelBackendConfig,
) -> Result<IndexRefreshOutcome, ApiError> {
    let requested_kinds =
        IndexRefreshPlan::from_requested(kinds.into_iter().collect()).into_kinds();
    let refreshable_kinds = requested_kinds
        .iter()
        .copied()
        .filter(|kind| read_models.refreshes_index(*kind))
        .collect::<Vec<_>>();
    if !refreshable_kinds.is_empty() {
        queue_index_refreshes(
            store.as_ref(),
            refreshable_kinds.clone(),
            graph_version,
            queue_policy,
        )
        .await?;
        drain_index_refresh_queue(store, read_models).await?;
    }

    let outcome = index_refresh_outcome(store).await?;

    Ok(filter_outcome_to_kinds(outcome, &refreshable_kinds))
}

pub(super) async fn reconcile_index_refreshes(
    store: &Arc<dyn KnowledgeStore>,
    graph_version: GraphVersion,
    read_models: &ReadModelBackendConfig,
) -> Result<IndexRefreshDiagnostics, ApiError> {
    let refreshable_kinds = IndexKind::ALL
        .into_iter()
        .filter(|kind| read_models.refreshes_index(*kind))
        .collect::<Vec<_>>();
    if refreshable_kinds.is_empty() {
        return store
            .index_refresh_diagnostics(now_millis())
            .await
            .map(|diagnostics| filter_diagnostics_to_kinds(diagnostics, &refreshable_kinds))
            .map_err(storage_api_error);
    }

    let diagnostics = queue_index_refreshes(
        store.as_ref(),
        refreshable_kinds.clone(),
        graph_version,
        DIAGNOSTIC_RECONCILE_QUEUE,
    )
    .await?;

    Ok(filter_diagnostics_to_kinds(diagnostics, &refreshable_kinds))
}

pub(super) async fn index_refresh_outcome(
    store: &Arc<dyn KnowledgeStore>,
) -> Result<IndexRefreshOutcome, ApiError> {
    let indexes = store.index_statuses().await.map_err(storage_api_error)?;
    let cursors = store.index_cursors().await.map_err(storage_api_error)?;
    let diagnostics = store
        .index_refresh_diagnostics(now_millis())
        .await
        .map_err(storage_api_error)?;

    Ok(IndexRefreshOutcome {
        indexes,
        cursors,
        diagnostics,
    })
}

pub(super) fn metadata_for_indexes(
    context: &RequestContext,
    graph_version: GraphVersion,
    indexes: &[IndexStatus],
) -> ApiMetadata {
    let latest_index_version = indexes.iter().map(|status| status.index_version).max();
    let lowest_indexed_graph_version = indexes
        .iter()
        .map(|status| status.indexed_graph_version)
        .min();
    let stale = indexes
        .iter()
        .any(|status| status.is_stale_for(graph_version));

    ApiMetadata::indexed(
        context,
        graph_version,
        latest_index_version,
        lowest_indexed_graph_version,
        stale,
    )
}

pub(super) fn filter_outcome_to_read_models(
    outcome: IndexRefreshOutcome,
    read_models: &ReadModelBackendConfig,
) -> IndexRefreshOutcome {
    let refreshable_kinds = active_index_kinds(read_models);

    filter_outcome_to_kinds(outcome, &refreshable_kinds)
}

fn active_index_kinds(read_models: &ReadModelBackendConfig) -> Vec<IndexKind> {
    IndexKind::ALL
        .into_iter()
        .filter(|kind| read_models.refreshes_index(*kind))
        .collect()
}

fn filter_outcome_to_kinds(
    mut outcome: IndexRefreshOutcome,
    refreshable_kinds: &[IndexKind],
) -> IndexRefreshOutcome {
    outcome
        .indexes
        .retain(|status| refreshable_kinds.contains(&status.kind));
    outcome
        .cursors
        .retain(|cursor| refreshable_kinds.contains(&cursor.kind));
    outcome.diagnostics = filter_diagnostics_to_kinds(outcome.diagnostics, refreshable_kinds);

    outcome
}

fn filter_diagnostics_to_kinds(
    mut diagnostics: IndexRefreshDiagnostics,
    refreshable_kinds: &[IndexKind],
) -> IndexRefreshDiagnostics {
    diagnostics
        .index_lag_by_kind
        .retain(|lag| refreshable_kinds.contains(&lag.kind));
    diagnostics.max_index_lag_versions = diagnostics
        .index_lag_by_kind
        .iter()
        .map(|lag| lag.lag_versions)
        .max()
        .unwrap_or(0);
    diagnostics
        .stale_reasons
        .retain(|reason| refreshable_kinds.contains(&reason.kind));
    diagnostics.stale_index_count = diagnostics
        .stale_reasons
        .iter()
        .filter(|reason| reason.source_scope.is_none() && reason.modality.is_none())
        .map(|reason| reason.kind)
        .fold(Vec::new(), |mut kinds, kind| {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
            kinds
        })
        .len();

    diagnostics
}

fn storage_api_error(error: StorageError) -> ApiError {
    ApiError::storage_unavailable(error.to_string())
}

async fn queue_index_refreshes(
    store: &dyn IndexStore,
    kinds: impl IntoIterator<Item = IndexKind>,
    graph_version: GraphVersion,
    queue_policy: QueuePolicy,
) -> Result<IndexRefreshDiagnostics, ApiError> {
    let kinds = kinds.into_iter().collect::<Vec<_>>();
    let mut max_queue_depth = INITIAL_QUEUE_DEPTH;
    loop {
        let now_ms = now_millis();
        match store
            .queue_index_refreshes(IndexRefreshQueueRequest {
                kinds: kinds.clone(),
                target_graph_version: graph_version,
                max_queue_depth,
                reset_dead_letter_tasks: queue_policy.reset_dead_letter_tasks,
                now_ms,
            })
            .await
        {
            Ok(diagnostics) => return Ok(diagnostics),
            Err(error) if is_queue_capacity_error(&error) => {
                if max_queue_depth >= MAX_QUEUE_DEPTH {
                    if queue_policy.capacity == QueueCapacityPolicy::DiagnosticsOnly {
                        return store
                            .index_refresh_diagnostics(now_ms)
                            .await
                            .map_err(storage_api_error);
                    }
                    return Err(storage_api_error(error));
                }
                max_queue_depth = max_queue_depth.saturating_mul(2).min(MAX_QUEUE_DEPTH);
            }
            Err(error) => return Err(storage_api_error(error)),
        }
    }
}

async fn drain_index_refresh_queue(
    store: &Arc<dyn KnowledgeStore>,
    read_models: &ReadModelBackendConfig,
) -> Result<(), ApiError> {
    let lease_owner = format!("foreground-refresh-{}", std::process::id());
    let mut first_failure = None;
    loop {
        let now_ms = now_millis();
        let Some(task) = store
            .claim_index_refresh_task(IndexRefreshClaimRequest {
                lease_owner: lease_owner.clone(),
                lease_duration_ms: LEASE_DURATION_MS,
                max_attempts: MAX_ATTEMPTS,
                now_ms,
            })
            .await
            .map_err(storage_api_error)?
        else {
            return match first_failure {
                Some(error) => Err(error),
                None => Ok(()),
            };
        };

        if let Err(error) = replay_mutations_for_task(store, &task).await {
            let failure = store
                .fail_index_refresh_task(IndexRefreshFailure {
                    task_id: task.task_id.clone(),
                    lease_owner: lease_owner.clone(),
                    attempt_count: task.attempt_count,
                    error_kind: "refresh_failed".to_owned(),
                    error_message: error.message.clone(),
                    retry_backoff_ms: RETRY_BACKOFF_MS,
                    max_attempts: MAX_ATTEMPTS,
                    now_ms: now_millis(),
                })
                .await
                .map_err(storage_api_error)?;
            first_failure.get_or_insert_with(|| {
                ApiError::storage_unavailable(format!(
                    "index refresh task {} failed in state {}: {}",
                    failure.task_id,
                    failure.state.as_str(),
                    error.message
                ))
            });
            continue;
        }

        let (model_name, model_dimension) = refresh_model_metadata(task.kind, read_models);
        store
            .complete_index_refresh_task(IndexRefreshCompletion {
                task_id: task.task_id,
                lease_owner: lease_owner.clone(),
                attempt_count: task.attempt_count,
                indexed_graph_version: task.target_graph_version,
                model_name,
                model_dimension,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;
    }
}

fn refresh_model_metadata(
    kind: IndexKind,
    read_models: &ReadModelBackendConfig,
) -> (Option<String>, Option<u32>) {
    match kind {
        IndexKind::Bm25 => (None, None),
        IndexKind::Semantic => (
            Some(read_models.semantic_model.name.clone()),
            Some(read_models.semantic_model.dimension),
        ),
        IndexKind::Vector => (
            Some(read_models.vector_model.name.clone()),
            Some(read_models.vector_model.dimension),
        ),
    }
}

fn is_queue_capacity_error(error: &StorageError) -> bool {
    matches!(
        error,
        StorageError::InvalidInput(message)
            if message.contains("index refresh queue capacity exceeded")
    )
}

async fn replay_mutations_for_task(
    store: &Arc<dyn KnowledgeStore>,
    task: &IndexRefreshTask,
) -> Result<(), ApiError> {
    if task.cursor_before >= task.target_graph_version {
        return Ok(());
    }

    let mut cursor = task.cursor_before;
    let mut saw_target = false;
    let mut matched_mutations = 0usize;
    loop {
        let entries = store
            .read_after(cursor, MUTATION_LOG_PAGE_SIZE)
            .await
            .map_err(storage_api_error)?;
        if entries.is_empty() {
            break;
        }

        for entry in entries {
            if entry.graph_version > task.target_graph_version {
                saw_target = true;
                break;
            }
            if task_matches_entry(task, &entry) {
                matched_mutations += 1;
            }
            cursor = entry.graph_version;
            if cursor >= task.target_graph_version {
                saw_target = true;
                break;
            }
        }

        if saw_target {
            break;
        }
    }

    if cursor < task.target_graph_version {
        return Err(ApiError::storage_unavailable(format!(
            "mutation log ended at graph version {} before refresh target {}",
            cursor.get(),
            task.target_graph_version.get()
        )));
    }
    if matched_mutations == 0 && task.source_scope != DEFAULT_INDEX_SOURCE_SCOPE {
        return Err(ApiError::storage_unavailable(format!(
            "mutation log did not contain scope '{}' between graph versions {} and {}",
            task.source_scope,
            task.cursor_before.get(),
            task.target_graph_version.get()
        )));
    }

    Ok(())
}

fn task_matches_entry(task: &IndexRefreshTask, entry: &MutationLogEntry) -> bool {
    if task.source_scope == DEFAULT_INDEX_SOURCE_SCOPE
        || entry
            .affected_scopes
            .iter()
            .any(|scope| scope == &task.source_scope)
    {
        return true;
    }

    false
}

fn now_millis() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::IndexStatus,
        storage::{StorageError, StorageFuture},
    };

    struct CapacityLimitedIndexStore;

    impl IndexStore for CapacityLimitedIndexStore {
        fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
            Box::pin(async {
                Err(StorageError::InvalidInput(
                    "index statuses are unavailable".to_owned(),
                ))
            })
        }

        fn mark_refresh_complete(
            &self,
            _kind: IndexKind,
            _graph_version: GraphVersion,
        ) -> StorageFuture<'_, IndexStatus> {
            Box::pin(async {
                Err(StorageError::InvalidInput(
                    "index completion is unavailable".to_owned(),
                ))
            })
        }

        fn queue_index_refreshes(
            &self,
            request: IndexRefreshQueueRequest,
        ) -> StorageFuture<'_, IndexRefreshDiagnostics> {
            Box::pin(async move {
                Err(StorageError::InvalidInput(format!(
                    "index refresh queue capacity exceeded: depth={} new=1 capacity={}",
                    request.max_queue_depth, request.max_queue_depth
                )))
            })
        }

        fn index_refresh_diagnostics(
            &self,
            _now_ms: u64,
        ) -> StorageFuture<'_, IndexRefreshDiagnostics> {
            Box::pin(async {
                Ok(IndexRefreshDiagnostics {
                    queue_depth: MAX_QUEUE_DEPTH,
                    running_count: 0,
                    retrying_count: 0,
                    dead_letter_count: 0,
                    oldest_unfinished_age_ms: Some(1),
                    index_lag_by_kind: Vec::new(),
                    max_index_lag_versions: 1,
                    stale_index_count: 1,
                    stale_reasons: Vec::new(),
                })
            })
        }
    }

    #[tokio::test]
    async fn explicit_refresh_returns_error_when_queue_cap_blocks_enqueue() {
        let store = CapacityLimitedIndexStore;

        let error = queue_index_refreshes(
            &store,
            vec![IndexKind::Bm25],
            GraphVersion::new(1),
            EXPLICIT_REFRESH_QUEUE,
        )
        .await
        .expect_err("explicit refresh should surface queue capacity");

        assert!(error.message.contains("queue capacity exceeded"));
    }

    #[tokio::test]
    async fn diagnostic_reconcile_degrades_when_queue_cap_blocks_enqueue() {
        let store = CapacityLimitedIndexStore;

        let diagnostics = queue_index_refreshes(
            &store,
            vec![IndexKind::Bm25],
            GraphVersion::new(1),
            DIAGNOSTIC_RECONCILE_QUEUE,
        )
        .await
        .expect("diagnostic reconciler should return stale diagnostics");

        assert_eq!(diagnostics.queue_depth, MAX_QUEUE_DEPTH);
        assert_eq!(diagnostics.stale_index_count, 1);
    }
}
