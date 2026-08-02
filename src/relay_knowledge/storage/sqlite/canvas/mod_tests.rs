use crate::{
    domain::GraphVersion,
    storage::{GraphCanvasSelection, GraphCanvasStorageRequest, GraphStore},
};

#[tokio::test]
async fn canvas_rejects_limits_outside_storage_budget() {
    let store = crate::storage::SqliteGraphStore::open_in_memory().expect("store should open");

    let zero = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: None,
            query: None,
            graph_version: GraphVersion::ZERO,
            limit: 0,
        })
        .await
        .expect_err("zero limit should fail");
    assert!(zero.to_string().contains("limit must be positive"));

    let oversized = store
        .graph_canvas(GraphCanvasStorageRequest {
            selection: GraphCanvasSelection::Knowledge,
            source_scope: None,
            query: None,
            graph_version: GraphVersion::ZERO,
            limit: 1001,
        })
        .await
        .expect_err("oversized limit should fail");
    assert!(oversized.to_string().contains("limit must be at most 1000"));
}
