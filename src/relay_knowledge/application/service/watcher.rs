use std::{path::PathBuf, sync::Arc};

use crate::{
    api::ApiError,
    domain::CodeRepositoryStatus,
    watcher::{FileWatcher, WatchedRepository, WatcherHandle},
};

use super::{RelayKnowledgeService, storage_api_error};

impl RelayKnowledgeService {
    pub async fn start_code_repository_watcher(&self) -> Result<Option<WatcherHandle>, ApiError> {
        if !self.runtime.watcher.enabled {
            self.stop_code_repository_watcher().await;
            return Ok(None);
        }

        let mut guard = self.watcher.write().await;
        if let Some(handle) = guard.as_ref() {
            return Ok(Some(handle.clone()));
        }

        let store = self.store().await.map_err(storage_api_error)?;
        let repositories = store
            .list_code_repositories()
            .await
            .map_err(storage_api_error)?
            .into_iter()
            .map(|status| watched_repository_from_status(&status))
            .collect::<Vec<_>>();
        let queue_store = Arc::clone(&store);
        let handle = FileWatcher::new(self.runtime.watcher.clone())
            .start_with_sink(repositories, move |seed| {
                let store = Arc::clone(&queue_store);
                async move {
                    store
                        .queue_code_index_task(seed)
                        .await
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                }
            })
            .map_err(ApiError::storage_unavailable)?;
        *guard = Some(handle.clone());

        Ok(Some(handle))
    }

    pub async fn stop_code_repository_watcher(&self) {
        let handle = self.watcher.write().await.take();
        if let Some(handle) = handle {
            handle.request_shutdown();
        }
    }

    pub(crate) async fn refresh_watched_code_repository(
        &self,
        status: &CodeRepositoryStatus,
    ) -> bool {
        let Some(handle) = self.watcher.read().await.as_ref().cloned() else {
            return false;
        };
        handle
            .add_repository(watched_repository_from_status(status))
            .await
    }

    pub(crate) async fn remove_watched_code_repository(
        &self,
        alias: &str,
        repository_id: &str,
    ) -> bool {
        let Some(handle) = self.watcher.read().await.as_ref().cloned() else {
            return false;
        };
        handle.remove_repository(alias).await || handle.remove_repository(repository_id).await
    }

    pub(super) async fn watcher_diagnostics(&self) -> Option<crate::watcher::WatcherDiagnostics> {
        self.watcher
            .read()
            .await
            .as_ref()
            .map(WatcherHandle::diagnostics)
    }
}

fn watched_repository_from_status(status: &CodeRepositoryStatus) -> WatchedRepository {
    let source_scope = status
        .last_indexed_scope_id
        .clone()
        .unwrap_or_else(|| format!("watcher:{}", status.repository_id));
    WatchedRepository {
        repository_id: status.repository_id.clone(),
        alias: status.alias.clone(),
        root: PathBuf::from(status.root_path.clone()),
        path_filters: status.path_filters.clone(),
        language_filters: status.language_filters.clone(),
        source_scope,
    }
}
