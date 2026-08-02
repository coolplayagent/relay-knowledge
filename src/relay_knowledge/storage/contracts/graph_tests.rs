use super::*;

struct MinimalGraphStore;

impl GraphStore for MinimalGraphStore {
    fn commit_mutation_batch(
        &self,
        _batch: GraphMutationBatch,
    ) -> StorageFuture<'_, CommitReceipt> {
        Box::pin(async { panic!("required graph commit must not run in default contract tests") })
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        Box::pin(async {
            panic!("required graph inspection must not run in default contract tests")
        })
    }

    fn search(&self, _request: GraphSearchRequest) -> StorageFuture<'_, GraphSearchOutcome> {
        Box::pin(async { panic!("required graph search must not run in default contract tests") })
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        Box::pin(async { panic!("required graph version must not run in default contract tests") })
    }
}

#[tokio::test]
async fn default_optional_graph_methods_report_unavailable_storage() {
    let store = MinimalGraphStore;
    let health = store
        .health_snapshot(1)
        .await
        .expect_err("default health storage should be unavailable");
    let canvas = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: super::super::GraphCanvasSelection::Mixed,
            source_scope: None,
            query: None,
            graph_version: GraphVersion::ZERO,
            limit: 1,
        })
        .await
        .expect_err("default canvas storage should be unavailable");

    assert!(health.to_string().contains("health snapshot storage"));
    assert!(canvas.to_string().contains("graph canvas storage"));
}
