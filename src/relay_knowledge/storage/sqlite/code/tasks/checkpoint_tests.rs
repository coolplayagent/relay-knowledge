use rusqlite::params;

use super::{checkpoint, latest_checkpoint_for_repository};
use crate::{
    domain::{CodeIndexResourceBudget, CodeRepositoryRegistration},
    storage::{CodeRepositoryStore, SqliteGraphStore, StorageError},
};

#[tokio::test]
async fn checkpoint_reads_scope_and_latest_repository_progress_deterministically() {
    let store = registered_store().await;
    store
        .run(|connection| {
            insert_checkpoint(connection, "scope-a", 225)?;
            insert_checkpoint(connection, "scope-b", 225)
        })
        .await
        .expect("checkpoints should insert");

    let by_scope = store
        .run(|connection| checkpoint(connection, "scope-a"))
        .await
        .expect("checkpoint should query")
        .expect("checkpoint should exist");
    assert_eq!(by_scope.source_scope, "scope-a");
    assert_eq!(by_scope.committed_file_count, 1);

    let latest = store
        .run(|connection| latest_checkpoint_for_repository(connection, "repo"))
        .await
        .expect("latest checkpoint should query")
        .expect("latest checkpoint should exist");
    assert_eq!(latest.source_scope, "scope-b");
    assert_eq!(latest.updated_at_ms, 225);
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

fn insert_checkpoint(
    connection: &mut rusqlite::Connection,
    scope: &str,
    updated_at_ms: u64,
) -> Result<(), StorageError> {
    let resource_budget = serde_json::to_string(&CodeIndexResourceBudget::default())
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    connection.execute(
        "
        INSERT INTO code_repository_index_checkpoints (
            source_scope, repository_id, state, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, total_path_count, parsed_file_count,
            committed_file_count, committed_symbol_count, committed_reference_count,
            committed_chunk_count, batch_count, last_path, resource_budget_json,
            updated_at_ms, error_message
        )
        VALUES (?1, 'repo', 'complete', ?2, ?3, '[\"src\"]', '[\"rust\"]',
                1, 1, 1, 0, 0, 0, 1, 'src/lib.rs', ?4, ?5, NULL)
        ",
        params![
            scope,
            format!("commit-{scope}"),
            format!("tree-{scope}"),
            resource_budget,
            updated_at_ms,
        ],
    )?;
    Ok(())
}
