use std::path::{Path, PathBuf};

use super::WatchedRepository;

/// Builds the single durable reconciliation slot for a repository's checked-out ref.
///
/// The fingerprint intentionally excludes the moving commit pair: while one
/// commit update is unfinished, repeated hints coalesce into that task. Once it
/// publishes, the same slot can be reset with the next immutable pair.
pub fn build_commit_task_seed(
    repository: &WatchedRepository,
    base_commit: &str,
    head_commit: &str,
    tree_hash: &str,
    now_ms: u64,
) -> Option<crate::storage::CodeIndexTaskSeed> {
    if base_commit.is_empty()
        || head_commit.is_empty()
        || tree_hash.is_empty()
        || base_commit == head_commit
    {
        return None;
    }
    let mode = crate::domain::CodeIndexMode::incremental(base_commit, head_commit).ok()?;
    let request = crate::domain::CodeIndexRequest {
        repository: crate::domain::CodeRepositorySelector {
            repository: repository.alias.clone(),
            ref_selector: head_commit.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        },
        mode: mode.clone(),
        workspace_detection: Default::default(),
        freshness_policy: crate::domain::FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    };
    let mut payload = serde_json::to_value(&request).ok()?;
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "git_event".to_owned(),
            serde_json::json!({
                "kind": "ref_reconcile",
                "ref": "HEAD",
                "old_oid": base_commit,
                "new_oid": head_commit,
            }),
        );
    }
    let path_filters_json = serde_json::to_string(&repository.path_filters).ok()?;
    let language_filters_json = serde_json::to_string(&repository.language_filters).ok()?;
    let source_scope = crate::domain::code_snapshot_scope_id(
        &repository.repository_id,
        tree_hash,
        &repository.path_filters,
        &repository.language_filters,
    );

    Some(crate::storage::CodeIndexTaskSeed {
        repository_id: repository.repository_id.clone(),
        alias: repository.alias.clone(),
        ref_selector: "HEAD".to_owned(),
        resolved_commit_sha: head_commit.to_owned(),
        tree_hash: tree_hash.to_owned(),
        source_scope,
        path_filters: repository.path_filters.clone(),
        language_filters: repository.language_filters.clone(),
        mode,
        input_fingerprint: format!(
            "git_ref_reconcile:{}:HEAD:{}:{}",
            repository.repository_id, path_filters_json, language_filters_json
        ),
        resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        payload_json: serde_json::to_string(&payload).ok()?,
        now_ms,
    })
}

pub(super) struct ChangedPathSnapshot {
    pub path: PathBuf,
    pub content_hash: u64,
}

/// Builds a worktree task pinned to the repository's last clean indexed base.
pub fn build_incremental_task_seed(
    repository: &WatchedRepository,
    changed_paths: &[PathBuf],
    content_fingerprint: u64,
    now_ms: u64,
) -> Option<crate::storage::CodeIndexTaskSeed> {
    if changed_paths.is_empty() {
        return None;
    }
    let relative_paths = changed_path_labels(repository, changed_paths);
    if relative_paths.is_empty() {
        return None;
    }
    let base_commit = immutable_worktree_base(&repository.last_indexed_commit)?;
    let path_hash = stable_path_fingerprint(&relative_paths);
    let task_tree_hash = format!("worktree:pending:{base_commit}");
    let source_scope = crate::domain::code_snapshot_scope_id(
        &repository.repository_id,
        &task_tree_hash,
        &repository.path_filters,
        &repository.language_filters,
    );

    let input_fingerprint = format!(
        "worktree_overlay:{}:{}:{}:{path_hash:016x}:{content_fingerprint:016x}",
        repository.repository_id, task_tree_hash, source_scope,
    );

    let request = crate::domain::CodeIndexRequest {
        repository: crate::domain::CodeRepositorySelector {
            repository: repository.alias.clone(),
            ref_selector: base_commit.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        },
        mode: crate::domain::CodeIndexMode::WorktreeOverlay,
        workspace_detection: Default::default(),
        freshness_policy: crate::domain::FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    };
    let mut payload = serde_json::to_value(&request).ok()?;
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "watcher".to_owned(),
            serde_json::json!({
                "repository_id": repository.repository_id.clone(),
                "changed_paths": relative_paths,
                "content_fingerprint": format!("{content_fingerprint:016x}"),
            }),
        );
    }

    Some(crate::storage::CodeIndexTaskSeed {
        repository_id: repository.repository_id.clone(),
        alias: repository.alias.clone(),
        ref_selector: base_commit.to_owned(),
        resolved_commit_sha: task_tree_hash.clone(),
        tree_hash: task_tree_hash,
        source_scope,
        path_filters: repository.path_filters.clone(),
        language_filters: repository.language_filters.clone(),
        mode: crate::domain::CodeIndexMode::WorktreeOverlay,
        input_fingerprint,
        resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        payload_json: serde_json::to_string(&payload).ok()?,
        now_ms,
    })
}

/// Builds the stable periodic worktree reconciliation slot after a missed or
/// rejected file event. The worker still scans bounded Git status; the marker
/// is task metadata only and is never interpreted as a source path.
pub fn build_worktree_reconcile_task_seed(
    repository: &WatchedRepository,
    observation_fingerprint: u64,
    now_ms: u64,
) -> Option<crate::storage::CodeIndexTaskSeed> {
    let base_commit = immutable_worktree_base(&repository.last_indexed_commit)?;
    let task_tree_hash = format!("worktree:pending:{base_commit}");
    let source_scope = crate::domain::code_snapshot_scope_id(
        &repository.repository_id,
        &task_tree_hash,
        &repository.path_filters,
        &repository.language_filters,
    );
    let request = crate::domain::CodeIndexRequest {
        repository: crate::domain::CodeRepositorySelector {
            repository: repository.alias.clone(),
            ref_selector: base_commit.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        },
        mode: crate::domain::CodeIndexMode::WorktreeOverlay,
        workspace_detection: Default::default(),
        freshness_policy: crate::domain::FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    };
    let mut payload = serde_json::to_value(&request).ok()?;
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "watcher".to_owned(),
            serde_json::json!({
                "kind": "periodic_worktree_reconcile",
                "observation_fingerprint": format!("{observation_fingerprint:016x}"),
            }),
        );
    }
    Some(crate::storage::CodeIndexTaskSeed {
        repository_id: repository.repository_id.clone(),
        alias: repository.alias.clone(),
        ref_selector: base_commit.to_owned(),
        resolved_commit_sha: task_tree_hash.clone(),
        tree_hash: task_tree_hash,
        source_scope,
        path_filters: repository.path_filters.clone(),
        language_filters: repository.language_filters.clone(),
        mode: crate::domain::CodeIndexMode::WorktreeOverlay,
        input_fingerprint: format!(
            "worktree_reconcile:{}:{}:{observation_fingerprint:016x}",
            repository.repository_id, base_commit,
        ),
        resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        payload_json: serde_json::to_string(&payload).ok()?,
        now_ms,
    })
}

fn immutable_worktree_base(snapshot_identity: &str) -> Option<&str> {
    crate::domain::clean_git_commit_from_snapshot_identity(snapshot_identity)
}

pub(super) fn changed_content_fingerprint(
    repository: &WatchedRepository,
    changes: &[&ChangedPathSnapshot],
) -> u64 {
    let mut entries = changes
        .iter()
        .filter_map(|change| {
            let relative = change.path.strip_prefix(&repository.root).ok()?;
            let label = path_label(relative)?;
            Some((label, change.content_hash))
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries.dedup();
    stable_content_fingerprint(&entries)
}

pub(super) fn unreadable_path_fingerprint(path: &Path) -> u64 {
    let label = path_label(path).unwrap_or_else(|| "<unreadable>".to_owned());
    stable_content_fingerprint(&[(label, 0)])
}

fn changed_path_labels(repository: &WatchedRepository, changed_paths: &[PathBuf]) -> Vec<String> {
    let mut labels = changed_paths
        .iter()
        .filter_map(|path| path.strip_prefix(&repository.root).ok())
        .filter_map(path_label)
        .collect::<Vec<_>>();
    labels.sort();
    labels.dedup();
    labels
}

fn path_label(path: &Path) -> Option<String> {
    let value = path
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    (!value.is_empty()).then_some(value)
}

fn stable_path_fingerprint(paths: &[String]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for path in paths {
        for byte in path.as_bytes().iter().copied().chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

fn stable_content_fingerprint(entries: &[(String, u64)]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for (path, content_hash) in entries {
        for byte in path
            .as_bytes()
            .iter()
            .copied()
            .chain([0])
            .chain(content_hash.to_le_bytes())
            .chain([0])
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
