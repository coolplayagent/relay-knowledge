use super::queue_task;
use crate::{
    domain::{
        CodeIndexMode, CodeIndexResourceBudget, CodeIndexTaskState, CodeRepositoryRegistration,
    },
    storage::{CodeIndexTaskSeed, CodeRepositoryStore, SqliteGraphStore},
};

#[tokio::test]
async fn queue_reuses_unfinished_fingerprint_and_keeps_distinct_work_independent() {
    let store = registered_store().await;
    let first = store
        .run(|connection| queue_task(connection, seed("fp-a", "scope-a", 100)))
        .await
        .expect("task should queue");
    let duplicate = store
        .run(|connection| queue_task(connection, seed("fp-a", "scope-a", 101)))
        .await
        .expect("unfinished fingerprint should reuse task");
    let distinct = store
        .run(|connection| queue_task(connection, seed("fp-b", "scope-b", 101)))
        .await
        .expect("distinct fingerprint should queue");

    assert_eq!(first.task_id, duplicate.task_id);
    assert_ne!(first.task_id, distinct.task_id);
    assert_eq!(first.state, CodeIndexTaskState::Queued);
    assert_eq!(first.path_filters, ["src"]);
    assert_eq!(first.language_filters, ["rust"]);
    assert_eq!(first.mode, CodeIndexMode::Full);
}

async fn registered_store() -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                "repo",
                "fixture",
                "/tmp/repo",
                vec!["src".to_owned()],
                vec!["rust".to_owned()],
            )
            .expect("registration should validate"),
        )
        .await
        .expect("repository should persist");
    store
}

fn seed(fingerprint: &str, scope: &str, now_ms: u64) -> CodeIndexTaskSeed {
    CodeIndexTaskSeed {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: format!("commit-{scope}"),
        tree_hash: format!("tree-{scope}"),
        source_scope: scope.to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        mode: CodeIndexMode::Full,
        input_fingerprint: fingerprint.to_owned(),
        resource_budget: CodeIndexResourceBudget::default(),
        payload_json: "{}".to_owned(),
        now_ms,
    }
}
