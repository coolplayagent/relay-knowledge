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
