//! Coordinates synchronous and durable repository-set overlay refresh.

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositorySetRefreshResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::{CodeRepositorySetRefreshTaskRecord, CodeRepositorySetStatus},
    storage::{
        CodeRepositorySetMemberSeed, CodeRepositorySetRefreshPublication,
        CodeRepositorySetRefreshTaskClaimRequest, CodeRepositorySetRefreshTaskCompletion,
        CodeRepositorySetRefreshTaskFailure, CodeRepositorySetRefreshTaskSeed,
    },
};

use super::{
    super::clock::now_millis,
    errors::storage_api_error,
    member_freshness::fact_version_scope_mismatch_reason,
    status::{refreshed_required_set_status, required_set_status},
};

const REPOSITORY_SET_REFRESH_TASK_LEASE_MS: u64 = 10 * 60 * 1000;
const REPOSITORY_SET_REFRESH_TASK_MAX_ATTEMPTS: u32 = 3;
const REPOSITORY_SET_REFRESH_TASK_RETRY_BACKOFF_MS: u64 = 60_000;

struct RefreshTaskExecution {
    task: CodeRepositorySetRefreshTaskRecord,
    response: CodeRepositorySetRefreshResponse,
}

impl RelayKnowledgeService {
    /// Queues and, when available, synchronously drains one overlay refresh task.
    pub async fn refresh_code_repository_set(
        &self,
        set_alias: String,
        context: RequestContext,
    ) -> Result<CodeRepositorySetRefreshResponse, ApiError> {
        let queued = self
            .start_code_repository_set_refresh(set_alias, context.clone())
            .await?;
        let task_id = queued.task.as_ref().map(|task| task.task_id.clone());
        match self
            .execute_code_repository_set_refresh_task(task_id, context)
            .await?
        {
            Some(execution) => Ok(execution.response),
            None => Ok(queued),
        }
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
        self.execute_code_repository_set_refresh_task(task_id, context)
            .await
            .map(|execution| execution.map(|execution| execution.task))
    }

    async fn execute_code_repository_set_refresh_task(
        &self,
        task_id: Option<String>,
        context: RequestContext,
    ) -> Result<Option<RefreshTaskExecution>, ApiError> {
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
            .build_and_publish_code_repository_set_refresh(&task, &lease_owner, context)
            .await;
        match result {
            Ok(mut response) => {
                let completed = store
                    .complete_code_repository_set_refresh_task(
                        CodeRepositorySetRefreshTaskCompletion {
                            task_id: task.task_id,
                            lease_owner,
                            attempt_count: task.attempt_count,
                            now_ms: now_millis(),
                        },
                    )
                    .await
                    .map_err(storage_api_error)?;
                response.task = Some(completed.clone());
                Ok(Some(RefreshTaskExecution {
                    task: completed,
                    response,
                }))
            }
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

    async fn build_and_publish_code_repository_set_refresh(
        &self,
        task: &CodeRepositorySetRefreshTaskRecord,
        lease_owner: &str,
        context: RequestContext,
    ) -> Result<CodeRepositorySetRefreshResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let (preflight_status, replacements) =
            refreshed_required_set_status(&store, &task.set_alias).await?;
        if let Some(reason) = preflight_status
            .members
            .iter()
            .find_map(fact_version_scope_mismatch_reason)
        {
            return Err(ApiError::invalid_argument(format!(
                "code repository set '{}' cannot refresh overlay: {reason}",
                task.set_alias
            )));
        }
        let summary = store
            .refresh_code_repository_set_overlay(
                task.set_alias.clone(),
                CodeRepositorySetRefreshPublication {
                    task_id: task.task_id.clone(),
                    set_id: task.set_id.clone(),
                    lease_owner: lease_owner.to_owned(),
                    attempt_count: task.attempt_count,
                    member_replacements: replacements
                        .into_iter()
                        .map(|member| CodeRepositorySetMemberSeed {
                            set_alias: task.set_alias.clone(),
                            repository_id: member.repository_id,
                            repository_alias: member.repository_alias,
                            ref_selector: member.ref_selector,
                            resolved_commit_sha: member.resolved_commit_sha,
                            source_scope: member.source_scope,
                            path_filters: member.path_filters,
                            language_filters: member.language_filters,
                            priority: member.priority,
                        })
                        .collect(),
                },
            )
            .await
            .map_err(storage_api_error)?;
        let status = required_set_status(&store, &task.set_alias).await?;
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
