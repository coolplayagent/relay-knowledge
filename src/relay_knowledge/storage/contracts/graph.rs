use serde::{Deserialize, Serialize};

use crate::domain::{CommitReceipt, GraphMutationBatch, GraphVersion};

use super::{
    GraphCanvasStorageRequest, GraphCanvasStorageSnapshot, GraphInspection, GraphSearchOutcome,
    GraphSearchRequest, HealthStorageSnapshot, StorageError, StorageFuture,
};

/// Graph fact persistence and query contract.
pub trait GraphStore: Send + Sync {
    fn commit_mutation_batch(&self, batch: GraphMutationBatch) -> StorageFuture<'_, CommitReceipt>;

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection>;

    fn health_snapshot(&self, _now_ms: u64) -> StorageFuture<'_, HealthStorageSnapshot> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "health snapshot storage is unavailable".to_owned(),
            ))
        })
    }

    fn graph_canvas(
        &self,
        _request: GraphCanvasStorageRequest,
    ) -> StorageFuture<'_, GraphCanvasStorageSnapshot> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "graph canvas storage is unavailable".to_owned(),
            ))
        })
    }

    fn search(&self, request: GraphSearchRequest) -> StorageFuture<'_, GraphSearchOutcome>;

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion>;
}

/// Mutation log contract consumed by reconcilers and indexers.
pub trait MutationLogStore: Send + Sync {
    fn read_after(
        &self,
        graph_version: GraphVersion,
        limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>>;
}

/// Mutation log entry returned for replay and index refresh planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationLogEntry {
    pub graph_version: GraphVersion,
    pub evidence_count: usize,
    pub entity_count: usize,
    pub relation_count: usize,
    pub claim_count: usize,
    pub event_count: usize,
    pub affected_scopes: Vec<String>,
    pub affected_entity_ids: Vec<String>,
    pub evidence_ids: Vec<String>,
    pub source_hashes: Vec<String>,
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
