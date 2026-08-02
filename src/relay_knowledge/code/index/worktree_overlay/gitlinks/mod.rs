use std::{collections::BTreeSet, fs, path::Path};

use crate::code::{
    CodeIndexError,
    source::{changes, git::git_bytes, gitlink as source_gitlink, layout as scope},
};

use super::super::MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS;
use super::{
    directories::{
        contains_git_metadata, worktree_directory_files, worktree_directory_is_expandable,
    },
    overlay_scope::WorktreeOverlayScope,
    recording::{WorktreeFileOutputs, record_file_as, record_status_marker},
};
use recorder::{WorktreeOverlayRecorder, record_previous_gitlink_child_deletions};
use state::{
    StagedPathKind, base_path_exists, staged_path_kind, submodule_worktree_head,
    submodule_worktree_parent_path,
};

pub(super) mod recorder;
mod state;

pub(super) fn record_deleted_gitlink_overlay(
    root: &Path,
    base_commit: &str,
    path: &str,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<bool, CodeIndexError> {
    let Some(base_gitlink_commit) =
        source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?
    else {
        return Ok(false);
    };
    let entries = bounded_submodule_path_entries(
        root,
        path,
        Some(base_commit),
        &base_gitlink_commit,
        recorder.scope,
    )?;
    if entries.is_empty() {
        let retained_paths = BTreeSet::new();
        let recorded = record_previous_gitlink_child_deletions(
            path,
            recorder.previous_hashes,
            recorder.scope,
            &retained_paths,
            recorder.overlay_hash_input,
            recorder.deleted_paths,
        )?;
        if !recorded && submodule_path_scope_overlaps(path, recorder.scope) {
            record_status_marker(path, recorder.overlay_hash_input);
        }
        return Ok(true);
    }
    for entry in entries {
        if recorder.path_is_selected(&entry.parent_path) {
            recorder.record_deleted_path(&entry.parent_path);
        }
    }

    Ok(true)
}

pub(super) fn record_staged_gitlink_overlay(
    change: &changes::WorktreePathChange,
    root: &Path,
    base_commit: &str,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<bool, CodeIndexError> {
    if !change.has_index_change() {
        return Ok(false);
    }
    let path = &change.path;
    let base_gitlink = source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?;
    let Some(staged_kind) = staged_path_kind(root, path)? else {
        if let Some(base_gitlink_commit) = base_gitlink {
            record_base_gitlink_child_deletions(
                root,
                path,
                base_commit,
                &base_gitlink_commit,
                recorder,
            )?;
            return Ok(true);
        }
        return Ok(false);
    };
    let StagedPathKind::Gitlink(staged_commit) = staged_kind else {
        if let Some(base_gitlink_commit) = base_gitlink {
            record_base_gitlink_child_deletions(
                root,
                path,
                base_commit,
                &base_gitlink_commit,
                recorder,
            )?;
        }
        return Ok(false);
    };

    if change.has_worktree_change()
        && let Some(worktree_commit) = submodule_worktree_head(root, path)?
        && worktree_commit != staged_commit
    {
        record_gitlink_commit_overlay(root, base_commit, path, &worktree_commit, recorder)?;
        record_dirty_submodule_worktree_overlay(root, path, path, recorder)?;
        return Ok(true);
    }

    record_gitlink_commit_overlay(root, base_commit, path, &staged_commit, recorder)?;
    if change.has_worktree_change() {
        record_dirty_submodule_worktree_overlay(root, path, path, recorder)?;
    }

    Ok(true)
}

fn record_gitlink_commit_overlay(
    root: &Path,
    base_commit: &str,
    path: &str,
    gitlink_commit: &str,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<(), CodeIndexError> {
    let base_gitlink = source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?;
    let staged_entries =
        bounded_submodule_path_entries(root, path, None, gitlink_commit, recorder.scope)?;
    let staged_entries_are_empty = staged_entries.is_empty();
    if let Some(base_gitlink_commit) = base_gitlink {
        let staged_paths = staged_entries
            .iter()
            .map(|entry| entry.parent_path.clone())
            .collect::<BTreeSet<_>>();
        record_missing_base_gitlink_child_deletions(
            root,
            path,
            base_commit,
            &base_gitlink_commit,
            &staged_paths,
            recorder,
        )?;
    } else if base_path_exists(root, base_commit, path)? && recorder.path_is_selected(path) {
        recorder.record_deleted_path(path);
    }

    if staged_entries_are_empty && submodule_path_scope_overlaps(path, recorder.scope) {
        record_status_marker(path, recorder.overlay_hash_input);
    }
    for entry in staged_entries {
        recorder.record_gitlink_file(root, path, gitlink_commit, &entry)?;
    }

    Ok(())
}

pub(super) fn record_unstaged_gitlink_overlay(
    root: &Path,
    base_commit: &str,
    path: &str,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<bool, CodeIndexError> {
    let Some(base_gitlink_commit) =
        source_gitlink::gitlink_commit_at_tree(root, base_commit, path)?
    else {
        return Ok(false);
    };
    let Some(worktree_commit) = submodule_worktree_head(root, path)? else {
        return Ok(false);
    };
    if worktree_commit == base_gitlink_commit {
        return record_dirty_submodule_worktree_overlay(root, path, path, recorder);
    }

    let worktree_entries =
        bounded_submodule_path_entries(root, path, None, &worktree_commit, recorder.scope)?;
    let worktree_entries_are_empty = worktree_entries.is_empty();
    let worktree_paths = worktree_entries
        .iter()
        .map(|entry| entry.parent_path.clone())
        .collect::<BTreeSet<_>>();
    record_missing_base_gitlink_child_deletions(
        root,
        path,
        base_commit,
        &base_gitlink_commit,
        &worktree_paths,
        recorder,
    )?;
    if worktree_entries_are_empty && submodule_path_scope_overlaps(path, recorder.scope) {
        record_status_marker(path, recorder.overlay_hash_input);
    }
    for entry in worktree_entries {
        recorder.record_gitlink_file(root, path, &worktree_commit, &entry)?;
    }
    record_dirty_submodule_worktree_overlay(root, path, path, recorder)?;

    Ok(true)
}

fn record_dirty_submodule_worktree_overlay(
    root: &Path,
    path: &str,
    indexed_path: &str,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<bool, CodeIndexError> {
    let submodule_root = match source_gitlink::submodule_root(root, path) {
        Ok(submodule_root) => submodule_root,
        Err(_) => return Ok(false),
    };
    let status = git_bytes(
        &submodule_root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let changes = changes::worktree_changed_paths(&status);
    if changes.is_empty() {
        return Ok(false);
    }
    for change in &changes {
        if let Some(deleted_path) = &change.deleted_source {
            let parent_deleted_path = submodule_worktree_parent_path(indexed_path, deleted_path);
            if recorder.path_is_selected(&parent_deleted_path) {
                recorder.record_deleted_path(&parent_deleted_path);
            }
        }
        let parent_path = submodule_worktree_parent_path(indexed_path, &change.path);
        if !recorder.path_scope_overlaps(&parent_path) {
            continue;
        }
        if change.is_untracked() && !recorder.untracked_path_is_selected(&parent_path) {
            continue;
        }
        record_dirty_submodule_path(
            &submodule_root,
            indexed_path,
            &change.path,
            &parent_path,
            change,
            recorder,
        )?;
    }

    Ok(true)
}

fn record_dirty_submodule_path(
    submodule_root: &Path,
    submodule_path: &str,
    child_path: &str,
    parent_path: &str,
    change: &changes::WorktreePathChange,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<(), CodeIndexError> {
    let metadata = match fs::symlink_metadata(submodule_root.join(child_path)) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if recorder.path_is_selected(parent_path) {
                recorder.record_deleted_path(parent_path);
            }
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let file_type = metadata.file_type();
    if file_type.is_file() && recorder.path_is_selected(parent_path) {
        let mut outputs = WorktreeFileOutputs {
            overlay_hash_input: &mut *recorder.overlay_hash_input,
            deleted_paths: &mut *recorder.deleted_paths,
            files_to_parse: &mut *recorder.files_to_parse,
            skipped_unchanged_count: &mut *recorder.skipped_unchanged_count,
        };
        return record_file_as(
            submodule_root,
            child_path,
            parent_path,
            recorder.previous_hashes,
            &mut outputs,
        );
    }
    if file_type.is_dir() && contains_git_metadata(submodule_root, Path::new(child_path))? {
        if record_dirty_submodule_worktree_overlay(
            submodule_root,
            child_path,
            parent_path,
            recorder,
        )? {
            return Ok(());
        }
    } else if file_type.is_dir()
        && change.is_untracked()
        && worktree_directory_is_expandable(submodule_root, child_path)?
    {
        for nested_path in worktree_directory_files(submodule_root, child_path)? {
            let parent_nested_path = submodule_worktree_parent_path(submodule_path, &nested_path);
            if recorder.untracked_path_is_selected(&parent_nested_path) {
                let mut outputs = WorktreeFileOutputs {
                    overlay_hash_input: &mut *recorder.overlay_hash_input,
                    deleted_paths: &mut *recorder.deleted_paths,
                    files_to_parse: &mut *recorder.files_to_parse,
                    skipped_unchanged_count: &mut *recorder.skipped_unchanged_count,
                };
                record_file_as(
                    submodule_root,
                    &nested_path,
                    &parent_nested_path,
                    recorder.previous_hashes,
                    &mut outputs,
                )?;
            }
        }
    } else if recorder.path_is_selected(parent_path) {
        recorder.record_unparseable_path(parent_path);
    } else if recorder.path_scope_overlaps(parent_path) {
        record_status_marker(parent_path, recorder.overlay_hash_input);
    }

    Ok(())
}

fn record_base_gitlink_child_deletions(
    root: &Path,
    path: &str,
    base_commit: &str,
    base_gitlink_commit: &str,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<(), CodeIndexError> {
    let mut recorded = false;
    for entry in bounded_submodule_path_entries(
        root,
        path,
        Some(base_commit),
        base_gitlink_commit,
        recorder.scope,
    )? {
        recorder.record_deleted_path(&entry.parent_path);
        recorded = true;
    }
    if !recorded {
        let retained_paths = BTreeSet::new();
        recorded = record_previous_gitlink_child_deletions(
            path,
            recorder.previous_hashes,
            recorder.scope,
            &retained_paths,
            recorder.overlay_hash_input,
            recorder.deleted_paths,
        )?;
    }
    if !recorded && submodule_path_scope_overlaps(path, recorder.scope) {
        record_status_marker(path, recorder.overlay_hash_input);
    }

    Ok(())
}

fn record_missing_base_gitlink_child_deletions(
    root: &Path,
    path: &str,
    base_commit: &str,
    base_gitlink_commit: &str,
    staged_paths: &BTreeSet<String>,
    recorder: &mut WorktreeOverlayRecorder<'_, '_>,
) -> Result<(), CodeIndexError> {
    let mut recorded = false;
    for entry in bounded_submodule_path_entries(
        root,
        path,
        Some(base_commit),
        base_gitlink_commit,
        recorder.scope,
    )? {
        if !staged_paths.contains(&entry.parent_path) {
            recorder.record_deleted_path(&entry.parent_path);
            recorded = true;
        }
    }
    if !recorded {
        recorded = record_previous_gitlink_child_deletions(
            path,
            recorder.previous_hashes,
            recorder.scope,
            staged_paths,
            recorder.overlay_hash_input,
            recorder.deleted_paths,
        )?;
    }
    if !recorded && submodule_path_scope_overlaps(path, recorder.scope) {
        record_status_marker(path, recorder.overlay_hash_input);
    }

    Ok(())
}

fn bounded_submodule_path_entries(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    commit: &str,
    scope: &WorktreeOverlayScope<'_>,
) -> Result<Vec<source_gitlink::SubmodulePathEntry>, CodeIndexError> {
    let Some(selection_filters) = scope.selection_path_filters.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(child_filters) =
        scope::submodule_child_scope_filters_from_filters(path, selection_filters)
    else {
        return Ok(Vec::new());
    };
    let entries = match source_gitlink::submodule_path_entries_with_child_filters(
        root,
        path,
        parent_commit,
        commit,
        &child_filters,
    ) {
        Ok(entries) => entries,
        Err(error) if source_gitlink::submodule_expansion_is_unavailable(&error) => Vec::new(),
        Err(error) => return Err(error),
    };
    let selected_entries = entries
        .into_iter()
        .filter(|entry| scope.selected(&entry.parent_path))
        .collect::<Vec<_>>();
    if selected_entries.len() > MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS {
        return Err(CodeIndexError::InvalidInput(format!(
            "gitlink path {path} expands to {} files; run a full code index so the work is checkpointed and batched",
            selected_entries.len()
        )));
    }

    Ok(selected_entries)
}

fn submodule_path_scope_overlaps(path: &str, scope: &WorktreeOverlayScope<'_>) -> bool {
    scope
        .selection_path_filters
        .as_ref()
        .is_some_and(|filters| {
            scope::submodule_child_scope_filters_from_filters(path, filters).is_some()
        })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
