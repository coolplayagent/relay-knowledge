//! SQLite adapter for software-global projection refresh and reads.

use crate::{
    domain::{
        CodeIndexPublicationFence, CodeSoftwareProjectionPhase, SoftwareGlobalProjection,
        SoftwareGlobalRequest,
    },
    storage::{SoftwareProjectionStore, StorageError, StorageFuture},
};

use super::{ensure_queryable_code_scope, lifecycle};
use crate::storage::sqlite::{SqliteGraphStore, software};

impl SoftwareProjectionStore for SqliteGraphStore {
    fn refresh_software_global_projection(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            software::refresh_projection(connection, &source_scope)
        })
    }

    fn refresh_software_global_projection_with_fence(
        &self,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        let this = self.clone();
        Box::pin(async move {
            let mut completed_phases = 0usize;
            loop {
                if completed_phases >= CodeSoftwareProjectionPhase::COUNT {
                    return Err(StorageError::Invariant(format!(
                        "software projection for scope '{source_scope}' exceeded its durable phase bound"
                    )));
                }
                let step_scope = source_scope.clone();
                let step_fence = fence.clone();
                let authority_path = this.publication_authority_path.clone();
                let advance = this
                    .run(move |connection| {
                        let guard = lifecycle::publication_fence::prepare_guard(
                            connection,
                            step_fence,
                            authority_path.as_deref(),
                        )?;
                        software::advance_fenced_projection(connection, &step_scope, &guard)
                    })
                    .await?;
                match advance {
                    software::FencedProjectionAdvance::Complete => break,
                    software::FencedProjectionAdvance::Pending { checkpoint_state } => {
                        completed_phases += 1;
                        tracing::debug!(
                            source_scope,
                            checkpoint_state,
                            "durable software projection phase committed"
                        );
                        // Keep the projection future pending between writer quanta so the
                        // application lease heartbeat can acquire the released writer.
                        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                    }
                }
            }
            this.run_read_snapshot(move |connection| {
                software::refreshed_fenced_projection(connection, &source_scope)
            })
            .await
        })
    }

    fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run_read_snapshot(move |connection| software::projection(connection, request))
    }

    fn software_global_projection_for_scope(
        &self,
        source_scope: String,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            software::projection_for_scope(connection, &source_scope, request)
        })
    }
}
