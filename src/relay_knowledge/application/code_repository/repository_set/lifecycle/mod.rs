//! Coordinates repository-set removal and member cleanup.

use crate::{
    api::{ApiMetadata, CodeRepositorySetRemoveResponse, RequestContext},
    domain::CodeRepositorySetRemoveMemberRequest,
};

use super::{errors::storage_api_error, status::required_set_status};
use crate::application::service::RelayKnowledgeService;

impl RelayKnowledgeService {
    /// Removes one member snapshot from a repository set and releases its retention pin.
    pub async fn remove_code_repository_set_member(
        &self,
        request: CodeRepositorySetRemoveMemberRequest,
        context: RequestContext,
    ) -> Result<CodeRepositorySetRemoveResponse, crate::api::ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let member = store
            .remove_code_repository_set_member(
                request.set_alias.clone(),
                request.repository_alias.clone(),
            )
            .await
            .map_err(storage_api_error)?;
        let status = required_set_status(&store, &request.set_alias).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetRemoveResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            request,
            member,
            status,
        })
    }
}
