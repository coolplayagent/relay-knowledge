use std::sync::Arc;

use crate::{
    domain::CodeRepositoryStatus,
    storage::{SqliteGraphStore, StorageError},
};

use super::catalog::mirror_repository_status;

pub(super) async fn mirror_status(
    control: &Arc<SqliteGraphStore>,
    status: CodeRepositoryStatus,
) -> Result<(), StorageError> {
    let control = Arc::clone(control);
    control
        .run(move |connection| mirror_repository_status(connection, &status))
        .await
}
