//! Coordinates synchronous and durable repository-set overlay refresh.

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositorySetRefreshResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::{CodeRepositorySetRefreshTaskRecord, CodeRepositorySetStatus},
    storage::{
        CodeRepositorySetRefreshTaskClaimRequest, CodeRepositorySetRefreshTaskCompletion,
        CodeRepositorySetRefreshTaskFailure, CodeRepositorySetRefreshTaskSeed,
    },
};

use super::{
    super::clock::now_millis,
    errors::storage_api_error,
    member_freshness::fact_version_scope_mismatch_reason,
    status::{
        persist_fact_version_member_replacements, refreshed_required_set_status,
        required_set_status,
    },
};

const REPOSITORY_SET_REFRESH_TASK_LEASE_MS: u64 = 10 * 60 * 1000;
const REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS: u32 = 3;
const REPOSITORY_SET_REFRESH_TASK_RETRY_BACKOFF_MS: u64 = 60_000;

impl RelayKnowledgeService {
    /// Rebuilds cross-repository import/module overlay edges.
    pub async fn refresh_code_repository_set(
        &self,
        set_alias: String,
        context: RequestContext,
    ) -> Result<CodeRepositorySetRefreshResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let (preflight_status, replacements) =
            refreshed_required_set_status(&store, &set_alias).await?;
        if let Some(reason) = preflight_status
            .members
            .iter()
            .find_map(fact_version_scope_mismatch_reason)
        {
            return Err(ApiError::invalid_argument(format!(
                "code repository set '{set_alias}' cannot refresh overlay: {reason}"
            )));
        }
        persist_fact_version_member_replacements(&store, &set_alias, &replacements).await?;
        let summary = store
            .refresh_code_repository_set_overlay(set_alias.clone(), now_millis())
            .await
            .map_err(storage_api_error)?;
        let status = required_set_status(&store, &set_alias).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetRefreshResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
            summary: Some(summary),
            task: None,
        })
    }

    /// Queues a repository-set overlay refresh task.
    pub async fn start_code_repository_set_refresh(
        &self,
        set_alias: String,
        context: RequestContext,
    ) -> Result<CodeRepositorySetRefreshResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_set_status(&store, &set_alias).await?;
        let fingerprint = repository_set_refresh_fingerprint(&status);
        let task = store
            .queue_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskSeed {
                set_id: status.repository_set.set_id.clone(),
                set_alias: status.repository_set.alias.clone(),
                input_fingerprint: fingerprint,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetRefreshResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
            summary: None,
            task: Some(task),
        })
    }

    /// Runs one queued repository-set overlay refresh task under a lease.
    pub async fn run_code_repository_set_refresh_task_once(
        &self,
        task_id: Option<String>,
        context: RequestContext,
    ) -> Result<Option<CodeRepositorySetRefreshTaskRecord>, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let lease_owner = format!("code-repository-set-refresh-worker-{}", std::process::id());
        let Some(task) = store
            .claim_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskClaimRequest {
                task_id,
                lease_owner: lease_owner.clone(),
                lease_duration_ms: REPOSITORY_SET_REFRESH_TASK_LEASE_MS,
                max_attempts: REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS,
                now_ms: now_millis(),
            })
            .await
            .map_err(storage_api_error)?
        else {
            return Ok(None);
        };
        let result = self
            .refresh_code_repository_set(task.set_alias.clone(), context)
            .await;
        match result {
            Ok(_) => store
                .complete_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskCompletion {
                    task_id: task.task_id,
                    lease_owner,
                    attempt_count: task.attempt_count,
                    now_ms: now_millis(),
                })
                .await
                .map(Some)
                .map_err(storage_api_error),
            Err(error) => {
                let _ = store
                    .fail_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskFailure {
                        task_id: task.task_id,
                        lease_owner,
                        attempt_count: task.attempt_count,
                        error_kind: "repository_set_overlay".to_owned(),
                        error_message: error.message.clone(),
                        retry_backoff_ms: REPOSITORY_SET_REFRESH_TASK_RETRY_BACKOFF_MS,
                        max_attempts: REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS,
                        now_ms: now_millis(),
                    })
                    .await;
                Err(error)
            }
        }
    }
}

fn repository_set_refresh_fingerprint(status: &CodeRepositorySetStatus) -> String {
    let mut parts = vec![status.repository_set.set_id.clone()];
    parts.extend(status.members.iter().map(|member| {
        format!(
            "{}:{}:{}:{}:{}",
            member.member.repository_id,
            member.member.source_scope,
            member.member.resolved_commit_sha,
            member.tree_hash,
            member.stale
        )
    }));
    parts.join("|")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
