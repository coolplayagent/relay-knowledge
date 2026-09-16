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
    identity::{RepositorySourceKind, source_kind},
};

#[derive(Debug, Clone)]
pub(in crate::code) struct RepositorySourceSnapshot {
    pub(in crate::code) skipped_paths: Vec<crate::code::source::path_io::SkippedSourcePath>,
    pub(in crate::code) content_hashes: std::collections::BTreeMap<String, String>,
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
                skipped_paths: Vec::new(),
                content_hashes: Default::default(),
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
    let mut skipped_paths = Vec::new();
    let files = filesystem_files(&root, &policy, &mut skipped_paths)?;
    let mut entries = Vec::with_capacity(files.len());
    let mut hash_paths = Vec::new();
    for file in files {
        let mut byte_count = 0;
        if policy.hash_includes_path(&file.path)
            && policy.language_allows_hash(&file.path)
            && policy.file_preset_allows_hash(&file.path)
        {
            byte_count = match filesystem_byte_count(&root, &file.path) {
                Ok(count) => count,
                Err(CodeIndexError::Io(error)) => {
                    skipped_paths.push(
                        crate::code::source::path_io::SkippedSourcePath::from_error(
                            &file.path,
                            crate::domain::CodePathKind::File,
                            crate::domain::CodePathIoOperation::Metadata,
                            error,
                        )?,
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            hash_paths.push(file.path.clone());
        }
        entries.push(GitTreeEntry {
            path: file.path,
            byte_count,
        });
    }
    let content_hashes =
        super::filesystem_hashes::index_content_hashes(&root, &hash_paths, &mut skipped_paths)?;
    entries.retain(|entry| {
        !skipped_paths
            .iter()
            .any(|skipped| skipped.covers(&entry.path))
    });
    std::fs::read_dir(&root)?;
    let tree_hash =
        super::filesystem_hashes::tree_hash_with_skipped(&content_hashes, &skipped_paths);

    Ok(RepositorySourceSnapshot {
        kind: RepositorySourceKind::FileSystem,
        skipped_paths,
        content_hashes,
        root,
        resolved_commit_sha: tree_hash.clone(),
        tree_hash,
        entries,
    })
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod tests;

/// Completes discovery-expanded paths without rereading admitted or failed source paths.
pub(in crate::code) fn complete_selected_filesystem_entries(
    root: &Path,
    entries: &mut Vec<GitTreeEntry>,
    hashes: &mut std::collections::BTreeMap<String, String>,
    skipped: &mut Vec<crate::code::source::path_io::SkippedSourcePath>,
) -> Result<(), CodeIndexError> {
    let mut missing = Vec::new();
    for entry in entries
        .iter_mut()
        .filter(|entry| !hashes.contains_key(&entry.path))
    {
        match filesystem_byte_count(root, &entry.path) {
            Ok(count) => {
                entry.byte_count = count;
                missing.push(entry.path.clone());
            }
            Err(CodeIndexError::Io(error)) => {
                skipped.push(crate::code::source::path_io::SkippedSourcePath::from_error(
                    &entry.path,
                    crate::domain::CodePathKind::File,
                    crate::domain::CodePathIoOperation::Metadata,
                    error,
                )?)
            }
            Err(error) => return Err(error),
        }
    }
    hashes.extend(super::filesystem_hashes::index_content_hashes(
        root, &missing, skipped,
    )?);
    entries.retain(|entry| hashes.contains_key(&entry.path));
    Ok(())
}
