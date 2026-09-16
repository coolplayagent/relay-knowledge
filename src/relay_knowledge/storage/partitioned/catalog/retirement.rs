//! Removes scope routes at completed, fenced retirement boundaries.

use super::{SqliteShardCatalog, open_catalog_connection};
use crate::{domain::CodeIndexPublicationFence, storage::StorageError};
use rusqlite::params;

impl SqliteShardCatalog {
    pub(in crate::storage::partitioned) async fn remove_scope_route(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> Result<usize, StorageError> {
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            let connection = open_catalog_connection(&control_path)?;
            connection
                .execute(
                    "DELETE FROM storage_repository_shard_scopes
                     WHERE repository_id = ?1 AND source_scope = ?2",
                    params![repository_id, source_scope],
                )
                .map_err(StorageError::from)
        })
        .await?
    }

    /// Removes only this live attempt's unpublished route after its shard rows are gone.
    pub(in crate::storage::partitioned) async fn remove_abandoned_scope_route(
        &self,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> Result<(), StorageError> {
        let control_path = self.control_path.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = open_catalog_connection(&control_path)?;
            let guard = crate::storage::sqlite::code::lifecycle::publication_fence::prepare_guard(
                &connection,
                fence.clone(),
                None,
            )?;
            let transaction = connection.transaction()?;
            guard.validate_target_scope(&transaction, &source_scope)?;
            guard.validate(&transaction)?;
            transaction.execute(
                "DELETE FROM storage_repository_shard_scopes
                 WHERE source_scope = ?1 AND repository_id = ?2
                   AND state = 'staged' AND staged_task_id = ?3",
                params![source_scope, fence.repository_id, fence.task_id],
            )?;
            guard.validate(&transaction)?;
            transaction.commit()?;
            Ok(())
        })
        .await?
    }
}
