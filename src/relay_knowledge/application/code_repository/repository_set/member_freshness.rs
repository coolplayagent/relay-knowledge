use std::sync::Arc;

use crate::{
    api::ApiError,
    domain::{
        CodeRepositorySetMember, CodeRepositorySetMemberStatus, CodeRepositorySetStatus,
        CodeRepositoryStatus, code_snapshot_expected_scope_id,
    },
    storage::KnowledgeStore,
};

use super::service::storage_api_error;

pub(super) async fn refresh_fact_version_member_freshness(
    store: &Arc<dyn KnowledgeStore>,
    status: &mut CodeRepositorySetStatus,
) -> Result<Vec<CodeRepositorySetMember>, ApiError> {
    let mut replacements = Vec::new();
    for member_status in &mut status.members {
        if let Some(member) = refresh_member_fact_version_status(store, member_status).await? {
            replacements.push(member);
        }
    }

    Ok(replacements)
}

async fn refresh_member_fact_version_status(
    store: &Arc<dyn KnowledgeStore>,
    member_status: &mut CodeRepositorySetMemberStatus,
) -> Result<Option<CodeRepositorySetMember>, ApiError> {
    if member_scope_matches_current_fact_version(member_status) {
        return Ok(None);
    }
    let previous_scope = member_status.member.source_scope.clone();
    let current = store
        .code_repository_scope_status(
            member_status.member.repository_alias.clone(),
            member_status.member.resolved_commit_sha.clone(),
            member_status.member.path_filters.clone(),
            member_status.member.language_filters.clone(),
        )
        .await
        .map_err(storage_api_error)?;
    if let Some(current) = current {
        apply_current_fact_version_scope(member_status, current, &previous_scope);
        return Ok(Some(member_status.member.clone()));
    } else {
        mark_member_stale(
            member_status,
            format!(
                "repository set member '{}' scope '{}' no longer matches the current code fact version",
                member_status.member.repository_alias, previous_scope
            ),
        );
    }

    Ok(None)
}

fn apply_current_fact_version_scope(
    member_status: &mut CodeRepositorySetMemberStatus,
    current: CodeRepositoryStatus,
    previous_scope: &str,
) {
    let Some(current_scope) = current.last_indexed_scope_id.clone() else {
        return;
    };
    member_status.member.source_scope = current_scope.clone();
    member_status.indexed_path_filters = current.path_filters;
    member_status.indexed_language_filters = current.language_filters;
    if let Some(commit) = current.last_indexed_commit {
        member_status.member.resolved_commit_sha = commit;
    }
    if let Some(tree_hash) = current.tree_hash {
        member_status.tree_hash = tree_hash;
    }
    member_status.indexed_file_count = current.indexed_file_count;
    member_status.symbol_count = current.symbol_count;
    member_status.reference_count = current.reference_count;
    member_status.chunk_count = current.chunk_count;
    member_status.degraded_reason = current.degraded_reason;
    mark_member_stale(
        member_status,
        format!(
            "repository set member '{}' stored scope '{}' no longer matches the current code fact version; using current scope '{}'",
            member_status.member.repository_alias, previous_scope, current_scope
        ),
    );
}

pub(super) fn fact_version_scope_mismatch_reason(
    member_status: &CodeRepositorySetMemberStatus,
) -> Option<String> {
    (!member_scope_matches_current_fact_version(member_status)).then(|| {
        format!(
            "repository set member '{}' scope '{}' no longer matches the current code fact version",
            member_status.member.repository_alias, member_status.member.source_scope
        )
    })
}

pub(super) fn member_scope_matches_current_fact_version(
    member_status: &CodeRepositorySetMemberStatus,
) -> bool {
    if !is_generated_git_snapshot_scope(&member_status.member.source_scope) {
        return true;
    }
    code_snapshot_expected_scope_id(
        &member_status.member.repository_id,
        &member_status.tree_hash,
        &member_status.indexed_path_filters,
        &member_status.indexed_language_filters,
    )
    .is_some_and(|expected| expected == member_status.member.source_scope)
}

fn is_generated_git_snapshot_scope(source_scope: &str) -> bool {
    let Some(hash) = source_scope.strip_prefix("git_snapshot:") else {
        return false;
    };
    hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn mark_member_stale(member_status: &mut CodeRepositorySetMemberStatus, reason: String) {
    member_status.stale = true;
    member_status.freshness_state = "stale".to_owned();
    member_status.degraded_reason = append_reason(member_status.degraded_reason.take(), reason);
}

fn append_reason(existing: Option<String>, reason: String) -> Option<String> {
    match existing {
        Some(existing) if existing.contains(&reason) => Some(existing),
        Some(existing) => Some(format!("{existing}; {reason}")),
        None => Some(reason),
    }
}
