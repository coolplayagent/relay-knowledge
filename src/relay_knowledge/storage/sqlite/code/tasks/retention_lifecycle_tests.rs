use rusqlite::params;

use super::super::queue_task;
use super::{prune_scopes, retention_status};
use crate::{
    domain::{CodeIndexMode, CodeIndexResourceBudget, CodeRepositoryRegistration},
    storage::{
        CodeIndexTaskSeed, CodeRepositoryStore, CodeScopeRetentionRequest, SqliteGraphStore,
    },
};

#[tokio::test]
async fn code_scope_retention_prunes_only_non_retained_scopes() {
    let store = registered_store().await;
    store
        .run(|connection| {
            for (scope, updated_at) in [
                ("scope-old", 10_u64),
                ("scope-one", 100),
                ("scope-two", 200),
                ("scope-active", 300),
            ] {
                insert_scope(connection, scope)?;
                insert_checkpoint(connection, scope, updated_at)?;
            }
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = 'scope-active',
                     last_indexed_commit = 'commit-active',
                     tree_hash = 'tree-active'
                 WHERE repository_id = 'repo'",
                [],
            )?;
            queue_task(connection, seed("fp-unfinished", "scope-two", 400))?;
            Ok(())
        })
        .await
        .expect("fixtures should insert");

    let retention = store
        .run(|connection| retention_status(connection, "repo"))
        .await
        .expect("retention status should query");
    assert!(
        retention
            .retained_scopes
            .contains(&"scope-active".to_owned())
    );
    assert!(retention.retained_scopes.contains(&"scope-two".to_owned()));

    let mut retired_scopes = Vec::new();
    let mut initial_prunable_count = 0;
    for pass_index in 0..128 {
        let pass = store
            .run(|connection| {
                prune_scopes(
                    connection,
                    CodeScopeRetentionRequest {
                        repository_id: "repo".to_owned(),
                        active_scope: "scope-active".to_owned(),
                        retain_recent_successful_scopes: 1,
                        repository_retention_cutoff_ms: None,
                        repository_retention_cutoff_generation: None,
                        repository_retention_initial_scope: None,
                    },
                )
            })
            .await
            .expect("prune should run");
        if pass_index == 0 {
            initial_prunable_count = pass.prunable_scope_count;
        }
        retired_scopes.extend(pass.pruned_scopes);
        if !pass.maintenance_pending {
            break;
        }
    }
    retired_scopes.sort();
    assert_eq!(retired_scopes, ["scope-old", "scope-one"]);
    assert_eq!(initial_prunable_count, 2);

    let remaining = store
        .run(|connection| {
            let scope_count =
                connection.query_row("SELECT COUNT(*) FROM code_repository_scopes", [], |row| {
                    row.get::<_, usize>(0)
                })?;
            let old_checkpoint_count = connection.query_row(
                "SELECT COUNT(*) FROM code_repository_index_checkpoints
                 WHERE source_scope = 'scope-old'",
                [],
                |row| row.get::<_, usize>(0),
            )?;
            Ok((scope_count, old_checkpoint_count))
        })
        .await
        .expect("remaining rows should query");
    assert_eq!(remaining, (2, 0));
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

fn insert_scope(
    connection: &mut rusqlite::Connection,
    scope: &str,
) -> Result<(), crate::storage::StorageError> {
    connection.execute(
        "INSERT INTO code_repository_scopes (
             source_scope, repository_id, resolved_commit_sha, tree_hash,
             path_filters_json, language_filters_json, indexed_file_count,
             symbol_count, reference_count, chunk_count, stale, degraded_reason
         ) VALUES (?1, 'repo', ?2, ?3, '[\"src\"]', '[\"rust\"]', 1, 0, 0, 0, 0, NULL)",
        params![scope, format!("commit-{scope}"), format!("tree-{scope}")],
    )?;
    connection.execute(
        "INSERT INTO code_repository_files (
             repository_id, source_scope, file_id, path, language_id, blob_hash,
             byte_len, line_count, parse_status, degraded_reason
         ) VALUES ('repo', ?1, ?2, 'src/lib.rs', 'rust', 'blob', 1, 1, 'parsed', NULL)",
        params![scope, format!("file-{scope}")],
    )?;
    Ok(())
}

fn insert_checkpoint(
    connection: &mut rusqlite::Connection,
    scope: &str,
    updated_at_ms: u64,
) -> Result<(), crate::storage::StorageError> {
    let resource_budget = serde_json::to_string(&CodeIndexResourceBudget::default())
        .map_err(|error| crate::storage::StorageError::InvalidInput(error.to_string()))?;
    connection.execute(
        "INSERT INTO code_repository_index_checkpoints (
             source_scope, repository_id, state, resolved_commit_sha, tree_hash,
             path_filters_json, language_filters_json, total_path_count, parsed_file_count,
             committed_file_count, committed_symbol_count, committed_reference_count,
             committed_chunk_count, batch_count, last_path, resource_budget_json,
             updated_at_ms, error_message
         ) VALUES (?1, 'repo', 'completed', ?2, ?3, '[\"src\"]', '[\"rust\"]',
                   1, 1, 1, 0, 0, 0, 1, 'src/lib.rs', ?4, ?5, NULL)",
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
