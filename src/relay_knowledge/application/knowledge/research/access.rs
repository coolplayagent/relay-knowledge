//! Server-side selection never treats an arbitrary client path as root authority.
use crate::{
    api::ApiError,
    application::{FileIndexRootConfig, RelayKnowledgeService},
};
use serde::Deserialize;
use std::path::PathBuf;

/// A named repository or a file root/scope already authorized by server configuration.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResearchRootSelection {
    Repository { alias: String },
    Configured { path: PathBuf, source_scope: String },
}

impl RelayKnowledgeService {
    /// Reuses registered repository authority or an exact configured file root/scope.
    /// Selecting a configured archive never registers or indexes it.
    pub async fn authorized_research_root(
        &self,
        selection: ResearchRootSelection,
        scope: Option<&str>,
    ) -> Result<PathBuf, ApiError> {
        match selection {
            ResearchRootSelection::Repository { alias } => {
                self.registered_code_repository_root(&alias).await
            }
            ResearchRootSelection::Configured { path, source_scope } => {
                if !path.is_absolute() || scope.is_some_and(|scope| scope != source_scope) {
                    return Err(ApiError::invalid_argument(
                        "configured research root requires its exact authorized path and source scope",
                    ));
                }
                let requested = FileIndexRootConfig::new(source_scope, path);
                self.runtime
                    .file_index
                    .roots
                    .iter()
                    .find(|root| {
                        root.scope_id == requested.scope_id && root.root_path == requested.root_path
                    })
                    .map(|root| root.root_path.clone())
                    .ok_or_else(|| {
                        ApiError::invalid_argument(
                            "research root/scope is not configured by the server",
                        )
                    })
            }
        }
    }
}
