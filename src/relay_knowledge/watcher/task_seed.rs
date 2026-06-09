use std::path::{Path, PathBuf};

use super::WatchedRepository;

pub(super) struct ChangedPathSnapshot {
    pub path: PathBuf,
    pub content_hash: u64,
}

pub fn build_incremental_task_seed(
    repository: &WatchedRepository,
    changed_paths: &[PathBuf],
    ref_selector: &str,
    resolved_commit_sha: &str,
    tree_hash: &str,
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
    let path_hash = stable_path_fingerprint(&relative_paths);
    let effective_ref = if ref_selector.trim().is_empty() {
        "HEAD"
    } else {
        ref_selector
    };
    let task_resolved_commit = if resolved_commit_sha.trim().is_empty() {
        effective_ref.to_owned()
    } else {
        resolved_commit_sha.to_owned()
    };
    let task_tree_hash = if tree_hash.trim().is_empty() {
        format!("worktree:pending:{content_fingerprint:016x}")
    } else {
        tree_hash.to_owned()
    };

    let input_fingerprint = format!(
        "worktree_overlay:{}:{}:{}:{path_hash:016x}:{content_fingerprint:016x}",
        repository.repository_id, task_tree_hash, repository.source_scope,
    );

    let request = crate::domain::CodeIndexRequest {
        repository: crate::domain::CodeRepositorySelector {
            repository: repository.alias.clone(),
            ref_selector: effective_ref.to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        },
        mode: crate::domain::CodeIndexMode::WorktreeOverlay,
        workspace_detection: Default::default(),
        freshness_policy: crate::domain::FreshnessPolicy::WaitUntilFresh,
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
        ref_selector: effective_ref.to_owned(),
        resolved_commit_sha: task_resolved_commit,
        tree_hash: task_tree_hash,
        source_scope: repository.source_scope.clone(),
        path_filters: repository.path_filters.clone(),
        language_filters: repository.language_filters.clone(),
        mode: crate::domain::CodeIndexMode::WorktreeOverlay,
        input_fingerprint,
        resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        payload_json: serde_json::to_string(&payload).ok()?,
        now_ms,
    })
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
