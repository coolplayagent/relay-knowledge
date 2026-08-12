//! Assembles repository-set status and overlay freshness diagnostics.

use crate::{
    api::{ApiError, ApiMetadata, CodeRepositorySetStatusResponse, RequestContext},
    application::service::RelayKnowledgeService,
    domain::{CodeRepositorySelector, CodeRepositorySetMember, CodeRepositorySetStatus},
};

use super::{
    super::scope::resolve_code_ref_for_selector, errors::storage_api_error,
    member_freshness::refresh_fact_version_member_freshness,
};

impl RelayKnowledgeService {
    /// Returns repository-set freshness and member diagnostics.
    pub async fn code_repository_set_status(
        &self,
        set_alias: String,
        context: RequestContext,
    ) -> Result<CodeRepositorySetStatusResponse, ApiError> {
        let store = self.store().await.map_err(storage_api_error)?;
        let status = required_set_status(&store, &set_alias).await?;
        let graph_version = store
            .current_graph_version()
            .await
            .map_err(storage_api_error)?;

        Ok(CodeRepositorySetStatusResponse {
            metadata: ApiMetadata::graph_only(&context, graph_version),
            status,
        })
    }
}

pub(super) async fn required_set_status(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    set_alias: &str,
) -> Result<CodeRepositorySetStatus, ApiError> {
    refreshed_required_set_status(store, set_alias)
        .await
        .map(|(status, _)| status)
}

pub(super) async fn refreshed_required_set_status(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    set_alias: &str,
) -> Result<(CodeRepositorySetStatus, Vec<CodeRepositorySetMember>), ApiError> {
    let mut status = store
        .code_repository_set_status(set_alias.to_owned())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository set '{set_alias}' is not registered"
            ))
        })?;
    let fact_version_replacements =
        refresh_fact_version_member_freshness(store, &mut status).await?;
    refresh_moving_member_freshness(store, &mut status).await?;
    refresh_repository_set_freshness(&mut status);

    Ok((status, fact_version_replacements))
}

async fn refresh_moving_member_freshness(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    status: &mut CodeRepositorySetStatus,
) -> Result<(), ApiError> {
    for index in 0..status.members.len() {
        let member = status.members[index].member.clone();
        let Some(reason) = moving_member_stale_reason(store, &member).await? else {
            continue;
        };
        status.members[index].stale = true;
        status.members[index].freshness_state = "stale".to_owned();
        status.members[index].degraded_reason = Some(reason);
    }

    Ok(())
}

async fn moving_member_stale_reason(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    member: &crate::domain::CodeRepositorySetMember,
) -> Result<Option<String>, ApiError> {
    if !member_ref_tracks_repository(&member.ref_selector, &member.resolved_commit_sha) {
        return Ok(None);
    }
    let repository = store
        .code_repository_status(member.repository_id.clone())
        .await
        .map_err(storage_api_error)?
        .ok_or_else(|| {
            ApiError::invalid_argument(format!(
                "code repository '{}' is not registered",
                member.repository_alias
            ))
        })?;
    let ref_selector = member.ref_selector.clone();
    let selector = CodeRepositorySelector {
        repository: member.repository_alias.clone(),
        ref_selector: ref_selector.clone(),
        path_filters: member.path_filters.clone(),
        language_filters: member.language_filters.clone(),
    };
    let resolved = resolve_code_ref_for_selector(&repository, &selector, ref_selector).await;

    match resolved {
        Ok(current_commit) if current_commit == member.resolved_commit_sha => Ok(None),
        Ok(current_commit) => Ok(Some(format!(
            "repository set member '{}' ref '{}' now resolves to {}, not stored snapshot {}",
            member.repository_alias,
            member.ref_selector,
            current_commit,
            member.resolved_commit_sha
        ))),
        Err(error) => Ok(Some(format!(
            "repository set member '{}' ref '{}' could not be resolved: {error}",
            member.repository_alias,
            member.ref_selector,
            error = error.message
        ))),
    }
}

fn member_ref_tracks_repository(ref_selector: &str, resolved_commit_sha: &str) -> bool {
    let ref_selector = ref_selector.trim();
    !(ref_selector == resolved_commit_sha
        || (is_git_oid_prefix(ref_selector) && resolved_commit_sha.starts_with(ref_selector)))
}

fn is_git_oid_prefix(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn refresh_repository_set_freshness(status: &mut CodeRepositorySetStatus) {
    let member_stale = status.members.iter().any(|member| member.stale);
    if member_stale && !status.overlay.stale {
        status.overlay.stale = true;
        status.overlay.state = "overlay_stale".to_owned();
    }
    status.freshness_state = if status.members.is_empty() {
        "incomplete"
    } else if member_stale {
        "stale"
    } else if status.overlay.stale {
        "overlay_stale"
    } else {
        "fresh"
    }
    .to_owned();
    status.degraded_reason = status
        .members
        .iter()
        .find_map(|member| member.degraded_reason.clone())
        .or_else(|| status.overlay.degraded_reason.clone());
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
