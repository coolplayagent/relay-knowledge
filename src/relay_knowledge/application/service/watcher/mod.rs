use std::{path::PathBuf, sync::Arc};

use crate::{
    api::ApiError,
    domain::{CodeIndexTaskRecord, CodeIndexTaskState, CodeRepositoryStatus},
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
            .filter_map(|status| watched_repository_from_status(&status))
            .collect::<Vec<_>>();
        let queue_store = Arc::clone(&store);
        let handle = FileWatcher::new(self.runtime.watcher.clone())
            .start_with_sink(repositories, move |seed| {
                let store = Arc::clone(&queue_store);
                async move {
                    let queued = store
                        .queue_code_index_task(seed)
                        .await
                        .map_err(|error| error.to_string())?;
                    accept_watcher_task(queued)
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
        match watched_repository_from_status(status) {
            Some(repository) => handle.add_repository(repository).await,
            None => {
                handle.remove_repository(&status.alias).await
                    || handle.remove_repository(&status.repository_id).await
            }
        }
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

fn accept_watcher_task(task: CodeIndexTaskRecord) -> Result<(), String> {
    if task.state != CodeIndexTaskState::DeadLetter {
        return Ok(());
    }
    let failure = task
        .last_error_message
        .as_deref()
        .map(|message| format!(": {message}"))
        .unwrap_or_default();
    Err(format!(
        "durable code index task '{}' remains dead_letter{failure}; reset failed work before retrying repository '{}'",
        task.task_id, task.alias
    ))
}

fn watched_repository_from_status(status: &CodeRepositoryStatus) -> Option<WatchedRepository> {
    if status.stale {
        return None;
    }
    let source_scope = status.last_indexed_scope_id.clone()?;
    let last_indexed_commit = status.last_indexed_commit.clone()?;
    Some(WatchedRepository {
        repository_id: status.repository_id.clone(),
        alias: status.alias.clone(),
        root: PathBuf::from(status.root_path.clone()),
        path_filters: status.path_filters.clone(),
        language_filters: status.language_filters.clone(),
        source_scope,
        last_indexed_commit,
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
