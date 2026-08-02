use std::path::{Path, PathBuf};

use super::super::{
    CodeIndexError,
    changes::{GitTreeEntry, TrackedEntryScope, tracked_entries_state_with_scope},
    git::{resolve_ref, resolve_tree},
    ids::stable_hash64,
};
use super::{
    FileSystemScanPolicy,
    filesystem_access::{filesystem_byte_count, filesystem_files},
    filesystem_hashes::filesystem_tree_hash_for_paths,
    identity::{RepositorySourceKind, source_kind},
};

#[derive(Debug, Clone)]
pub(in crate::code) struct RepositorySourceSnapshot {
    pub(in crate::code) kind: RepositorySourceKind,
    pub(in crate::code) root: PathBuf,
    pub(in crate::code) resolved_commit_sha: String,
    pub(in crate::code) tree_hash: String,
    pub(in crate::code) entries: Vec<GitTreeEntry>,
}

pub(in crate::code) fn source_snapshot(
    root: &Path,
    ref_selector: &str,
    filesystem_policy: FileSystemScanPolicy,
) -> Result<RepositorySourceSnapshot, CodeIndexError> {
    match source_kind(root)? {
        RepositorySourceKind::Git => {
            let commit = resolve_ref(root, ref_selector)?;
            let parent_tree_hash = resolve_tree(root, &commit)?;
            let entry_scope = if filesystem_policy.path_scope_denied {
                TrackedEntryScope::empty()
            } else {
                TrackedEntryScope::from_path_filters(filesystem_policy.path_scope_filters())
            };
            let tracked = tracked_entries_state_with_scope(root, &commit, &entry_scope)?;
            let tree_hash =
                git_tree_hash_with_submodules(&parent_tree_hash, &tracked.submodule_states);
            Ok(RepositorySourceSnapshot {
                kind: RepositorySourceKind::Git,
                root: root.to_path_buf(),
                resolved_commit_sha: commit,
                tree_hash,
                entries: tracked.entries,
            })
        }
        RepositorySourceKind::FileSystem => filesystem_source_snapshot(root, filesystem_policy),
    }
}

pub(in crate::code) fn git_tree_hash_with_submodules(
    parent_tree_hash: &str,
    submodule_states: &[String],
) -> String {
    if submodule_states.is_empty() {
        return parent_tree_hash.to_owned();
    }

    let mut hash_input = Vec::new();
    hash_input.extend_from_slice(b"git-tree-with-submodules-v1\0");
    hash_input.extend_from_slice(parent_tree_hash.as_bytes());
    hash_input.push(0);
    for state in submodule_states {
        hash_input.extend_from_slice(state.as_bytes());
        hash_input.push(0);
    }

    format!("git_tree:{:016x}", stable_hash64(&hash_input))
}

pub(in crate::code) fn filesystem_source_snapshot(
    root: &Path,
    policy: FileSystemScanPolicy,
) -> Result<RepositorySourceSnapshot, CodeIndexError> {
    let root = root.canonicalize()?;
    let files = filesystem_files(&root, &policy)?;
    let mut entries = Vec::with_capacity(files.len());
    let mut hash_paths = Vec::new();
    for file in files {
        let byte_count = filesystem_byte_count(&root, &file.path)?;
        if policy.hash_includes_path(&file.path)
            && policy.language_allows_hash(&file.path)
            && policy.file_preset_allows_hash(&file.path)
        {
            hash_paths.push(file.path.clone());
        }
        entries.push(GitTreeEntry {
            path: file.path,
            byte_count,
        });
    }
    let tree_hash = filesystem_tree_hash_for_paths(&root, &hash_paths)?;

    Ok(RepositorySourceSnapshot {
        kind: RepositorySourceKind::FileSystem,
        root,
        resolved_commit_sha: tree_hash.clone(),
        tree_hash,
        entries,
    })
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;
