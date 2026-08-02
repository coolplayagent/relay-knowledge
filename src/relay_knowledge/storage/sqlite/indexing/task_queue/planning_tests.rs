use crate::{
    domain::{GraphVersion, IndexKind},
    storage::{IndexRefreshQueueRequest, SqliteGraphStore},
};

use super::*;
use crate::storage::sqlite::indexing::task_queue::test_support::commit_evidence;

#[tokio::test]
async fn plans_missing_scope_cursor_from_graph_mutation() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-plan", "docs", "Rust async storage").await;
    let request = IndexRefreshQueueRequest {
        kinds: vec![IndexKind::Bm25],
        target_graph_version: GraphVersion::new(1),
        max_queue_depth: 4,
        reset_dead_letter_tasks: false,
        now_ms: 10,
    };
    let guard = store.connection.lock().expect("connection should lock");

    let tasks = planned_tasks(&guard, &request).expect("tasks should plan");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].kind, IndexKind::Bm25);
    assert_eq!(tasks[0].source_scope, "docs");
    assert_eq!(tasks[0].cursor_before, GraphVersion::ZERO);
    assert_eq!(tasks[0].target_graph_version, GraphVersion::new(1));
}
