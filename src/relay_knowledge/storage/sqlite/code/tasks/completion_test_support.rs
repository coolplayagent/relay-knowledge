//! Durable publication fixtures shared by completion-contract tests.

use super::*;

pub(super) async fn publish_task_target(
    store: &SqliteGraphStore,
    task: &CodeIndexTaskRecord,
    with_indexing_checkpoint: bool,
) {
    let task = task.clone();
    store
        .run(move |connection| {
            connection.execute(
                "UPDATE code_repositories
                 SET last_indexed_scope_id = ?2, last_indexed_commit = ?3, tree_hash = ?4,
                     state = 'fresh', stale = 0
                 WHERE repository_id = ?1",
                rusqlite::params![
                    task.repository_id,
                    task.source_scope,
                    task.resolved_commit_sha,
                    task.tree_hash
                ],
            )?;
            connection.execute(
                "INSERT OR REPLACE INTO code_repository_scopes (
                    source_scope, repository_id, resolved_commit_sha, tree_hash,
                    path_filters_json, language_filters_json, indexed_file_count,
                    symbol_count, reference_count, chunk_count, stale, degraded_reason
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, 0, 0, NULL)",
                rusqlite::params![
                    task.source_scope,
                    task.repository_id,
                    task.resolved_commit_sha,
                    task.tree_hash,
                    "[\"src\"]",
                    "[\"rust\"]"
                ],
            )?;
            connection.execute(
                "INSERT OR REPLACE INTO code_repository_publication_receipts
                 VALUES (?1, ?2, ?3, ?4, 1)",
                rusqlite::params![
                    task.task_id,
                    task.repository_id,
                    task.source_scope,
                    task.publication_generation
                ],
            )?;
            connection.execute(
                "INSERT OR REPLACE INTO software_global_status (
                    source_scope, repository_id, projected_graph_version, stale,
                    component_count, sdk_usage_count
                 ) VALUES (?1, ?2, 1, 0, 0, 0)",
                rusqlite::params![task.source_scope, task.repository_id],
            )?;
            connection.execute(
                "INSERT OR REPLACE INTO business_knowledge_status (
                    source_scope, repository_id, resolved_commit_sha,
                    projected_graph_version, stale, source_count, domain_count,
                    term_count, mapping_count, projection_schema_version, last_error
                 ) VALUES (?1, ?2, ?3, 1, 0, 0, 0, 0, 0, 1, NULL)",
                rusqlite::params![
                    task.source_scope,
                    task.repository_id,
                    task.resolved_commit_sha
                ],
            )?;
            if with_indexing_checkpoint {
                connection.execute(
                    "INSERT INTO code_repository_index_checkpoints (
                        source_scope, repository_id, state, resolved_commit_sha, tree_hash,
                        path_filters_json, language_filters_json, total_path_count,
                        parsed_file_count, committed_file_count, committed_symbol_count,
                        committed_reference_count, committed_chunk_count, batch_count, last_path,
                        resource_budget_json, updated_at_ms, error_message
                     ) VALUES (?1, ?2, 'indexing', ?3, ?4, ?5, ?6, 0, 0, 0, 0, 0, 0, 0,
                        NULL, ?7, 1, NULL)",
                    rusqlite::params![
                        task.source_scope,
                        task.repository_id,
                        task.resolved_commit_sha,
                        task.tree_hash,
                        "[\"src\"]",
                        "[\"rust\"]",
                        serde_json::to_string(&task.resource_budget).map_err(|error| {
                            crate::storage::StorageError::InvalidInput(error.to_string())
                        })?
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .expect("publication fixture should persist");
}
