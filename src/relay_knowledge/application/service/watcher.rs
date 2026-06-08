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
            .filter_map(|status| watched_repository_from_status(&status))
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

fn watched_repository_from_status(status: &CodeRepositoryStatus) -> Option<WatchedRepository> {
    if status.stale {
        return None;
    }
    let source_scope = status.last_indexed_scope_id.clone()?;
    Some(WatchedRepository {
        repository_id: status.repository_id.clone(),
        alias: status.alias.clone(),
        root: PathBuf::from(status.root_path.clone()),
        path_filters: status.path_filters.clone(),
        language_filters: status.language_filters.clone(),
        source_scope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(last_indexed_scope_id: Option<&str>, stale: bool) -> CodeRepositoryStatus {
        CodeRepositoryStatus {
            repository_id: "repo-1".to_owned(),
            alias: "core".to_owned(),
            root_path: "/tmp/core".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: Vec::new(),
            last_indexed_scope_id: last_indexed_scope_id.map(str::to_owned),
            last_indexed_commit: None,
            tree_hash: None,
            state: "registered".to_owned(),
            indexed_file_count: 0,
            symbol_count: 0,
            reference_count: 0,
            chunk_count: 0,
            stale,
            degraded_reason: None,
        }
    }

    #[test]
    fn watched_repository_from_status_skips_unindexed_repositories() {
        assert!(watched_repository_from_status(&status(None, true)).is_none());
    }

    #[test]
    fn watched_repository_from_status_skips_stale_repositories() {
        assert!(watched_repository_from_status(&status(Some("scope-1"), true)).is_none());
    }

    #[test]
    fn watched_repository_from_status_uses_indexed_scope() {
        let watched =
            watched_repository_from_status(&status(Some("scope-1"), false)).expect("indexed repo");
        assert_eq!(watched.repository_id, "repo-1");
        assert_eq!(watched.alias, "core");
        assert_eq!(watched.source_scope, "scope-1");
        assert_eq!(watched.path_filters, vec!["src"]);
    }
}
