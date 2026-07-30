use std::path::{Path, PathBuf};

use super::{
    commands::{git_tree_entry, submodule_worktree_root_for_commit},
    paths::SubmodulePathEntry,
    target::{git_blob_bytes_with_submodules, git_dir_blob_bytes_with_submodules},
};
use crate::code::{
    CodeIndexError,
    source::changes::{
        TrackedEntryScope, submodule_git_dir, submodule_worktree_root,
        tracked_entries_from_git_dir_with_scope, tracked_entries_with_scope,
    },
};

#[cfg(test)]
#[path = "entries_tests.rs"]
mod tests;

pub(in crate::code) fn gitlink_commit_at_tree(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<String>, CodeIndexError> {
    Ok(git_tree_entry(root, commit, path)?
        .filter(|entry| entry.kind == "commit")
        .map(|entry| entry.object))
}

pub(in crate::code) fn submodule_path_entries_with_child_filters(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    commit: &str,
    child_filters: &[String],
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    submodule_path_entries_with_scope(
        root,
        path,
        parent_commit,
        commit,
        &TrackedEntryScope::from_entry_path_filters(child_filters.iter()),
    )
}

pub(super) fn submodule_path_entries_with_scope(
    root: &Path,
    path: &str,
    parent_commit: Option<&str>,
    commit: &str,
    scope: &TrackedEntryScope,
) -> Result<Vec<SubmodulePathEntry>, CodeIndexError> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let entries =
        if let Some(submodule_root) = submodule_worktree_root_for_commit(root, path, commit) {
            tracked_entries_with_scope(&submodule_root, commit, scope)?
        } else {
            tracked_entries_from_git_dir_with_scope(
                &submodule_git_dir(root, path, parent_commit, Some(commit))?,
                commit,
                scope,
            )?
        };

    Ok(entries
        .into_iter()
        .map(|entry| SubmodulePathEntry {
            parent_path: format!("{prefix}{}", entry.path),
            child_path: entry.path,
        })
        .collect())
}

pub(in crate::code) fn submodule_entry_bytes(
    root: &Path,
    path: &str,
    commit: &str,
    child_path: &str,
) -> Result<Vec<u8>, CodeIndexError> {
    if let Some(submodule_root) = submodule_worktree_root_for_commit(root, path, commit) {
        git_blob_bytes_with_submodules(&submodule_root, commit, child_path)
    } else {
        git_dir_blob_bytes_with_submodules(
            &submodule_git_dir(root, path, None, Some(commit))?,
            commit,
            child_path,
        )
    }
}

pub(in crate::code) fn submodule_root(root: &Path, path: &str) -> Result<PathBuf, CodeIndexError> {
    submodule_worktree_root(root, path)
}
