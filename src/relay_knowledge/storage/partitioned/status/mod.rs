//! Control-plane status mirroring after shard publication.

use std::sync::Arc;

use crate::{
    domain::{CodeIndexPublicationFence, CodeRepositoryStatus},
    storage::{SqliteGraphStore, StorageError},
};

use super::catalog::mirror_repository_status;

pub(super) async fn mirror_status(
    control: &Arc<SqliteGraphStore>,
    status: CodeRepositoryStatus,
) -> Result<(), StorageError> {
    let control = Arc::clone(control);
    control
        .run(move |connection| {
            let transaction = connection.transaction()?;
            mirror_repository_status(&transaction, &status)?;
            transaction.commit()?;
            Ok(())
        })
        .await
}

pub(super) async fn mirror_status_with_fence(
    control: &Arc<SqliteGraphStore>,
    status: CodeRepositoryStatus,
    fence: CodeIndexPublicationFence,
) -> Result<(), StorageError> {
    let control = Arc::clone(control);
    control
        .run(move |connection| {
            let guard = crate::storage::sqlite::code::lifecycle::publication_fence::prepare_guard(
                connection, fence, None,
            )?;
            guard.validate_repository(&status.repository_id)?;
            let transaction = connection.transaction()?;
            mirror_repository_status(&transaction, &status)?;
            guard.validate(&transaction)?;
            transaction.commit()?;
            Ok(())
        })
        .await
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
