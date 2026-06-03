use std::sync::Arc;

use crate::storage::{CodeRepositoryStore, SqliteGraphStore, StorageError};

use super::catalog::SqliteShardCatalog;

pub(super) async fn repository_store_for_selector(
    control: &Arc<SqliteGraphStore>,
    catalog: &SqliteShardCatalog,
    repository: String,
) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
    let Some(status) = control.code_repository_status(repository).await? else {
        return Ok(None);
    };

    catalog
        .existing_repository_store(status.repository_id)
        .await
}

pub(super) async fn source_scope_store(
    catalog: &SqliteShardCatalog,
    source_scope: String,
) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
    let Some(repository_id) = catalog.repository_for_scope(source_scope).await? else {
        return Ok(None);
    };

    catalog.existing_repository_store(repository_id).await
}

pub(super) async fn current_control_scope(
    control: &Arc<SqliteGraphStore>,
    repository_id: String,
) -> Result<Option<String>, StorageError> {
    Ok(control
        .code_repository_status(repository_id)
        .await?
        .and_then(|status| status.last_indexed_scope_id))
}

pub(super) fn is_missing_code_scope_error(error: &StorageError) -> bool {
    error.to_string().contains("has no index for ref")
}
