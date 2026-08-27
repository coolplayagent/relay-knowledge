//! Framework graph routing to the repository shard that owns the selected scope.

use crate::{
    domain::{FrameworkGraph, FrameworkGraphRequest},
    storage::{FrameworkGraphStore, StorageFuture},
};

use super::{
    PartitionedSqliteKnowledgeStore,
    routing::{is_missing_code_scope_error, repository_store_for_selector},
};

impl FrameworkGraphStore for PartitionedSqliteKnowledgeStore {
    fn search_framework_graph(
        &self,
        request: FrameworkGraphRequest,
    ) -> StorageFuture<'_, FrameworkGraph> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.clone(),
            )
            .await?
            {
                return match shard.search_framework_graph(request.clone()).await {
                    Ok(graph) => Ok(graph),
                    Err(error) if is_missing_code_scope_error(&error) => {
                        this.control.search_framework_graph(request).await
                    }
                    Err(error) => Err(error),
                };
            }
            this.control.search_framework_graph(request).await
        })
    }

    fn search_framework_graph_scope(
        &self,
        source_scope: String,
        request: FrameworkGraphRequest,
    ) -> StorageFuture<'_, FrameworkGraph> {
        super::routing::search_framework_graph_scope(self.clone(), source_scope, request)
    }
}
