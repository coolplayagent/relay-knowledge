use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::code::{CodeIndexError, source::gitlink as source_gitlink};

use super::{
    super::super::{MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS, ids::stable_content_hash},
    super::{
        overlay_scope::WorktreeOverlayScope,
        recording::{record_deleted_path, record_unparseable_path},
    },
};

pub(super) fn record_previous_gitlink_child_deletions(
    path: &str,
    previous_hashes: &BTreeMap<String, String>,
    scope: &WorktreeOverlayScope<'_>,
    retained_paths: &BTreeSet<String>,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
) -> Result<bool, CodeIndexError> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let paths = previous_hashes
        .keys()
        .filter(|previous_path| previous_path.starts_with(&prefix))
        .filter(|previous_path| !retained_paths.contains(*previous_path))
        .filter(|previous_path| scope.selected(previous_path))
        .cloned()
        .collect::<BTreeSet<_>>();
    source_gitlink::ensure_gitlink_expansion_budget(
        path,
        paths.len(),
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
    )?;
    for path in &paths {
        record_deleted_path(path, overlay_hash_input, deleted_paths);
    }

    Ok(!paths.is_empty())
}

pub(in crate::code::index::worktree_overlay) struct WorktreeOverlayRecorder<'a, 'scope> {
    pub(in crate::code::index::worktree_overlay) scope: &'a WorktreeOverlayScope<'scope>,
    pub(in crate::code::index::worktree_overlay) previous_hashes: &'a BTreeMap<String, String>,
    pub(in crate::code::index::worktree_overlay) overlay_hash_input: &'a mut Vec<u8>,
    pub(in crate::code::index::worktree_overlay) deleted_paths: &'a mut Vec<String>,
    pub(in crate::code::index::worktree_overlay) files_to_parse: &'a mut Vec<(String, Vec<u8>)>,
    pub(in crate::code::index::worktree_overlay) skipped_unchanged_count: &'a mut usize,
}

impl WorktreeOverlayRecorder<'_, '_> {
    pub(super) fn path_is_selected(&self, path: &str) -> bool {
        self.scope.selected(path)
    }

    pub(super) fn path_scope_overlaps(&self, path: &str) -> bool {
        self.scope.overlaps(path)
    }

    pub(super) fn untracked_path_is_selected(&self, path: &str) -> bool {
        self.scope.untracked_selected(path)
    }

    pub(super) fn record_deleted_path(&mut self, path: &str) {
        record_deleted_path(path, self.overlay_hash_input, self.deleted_paths);
    }

    pub(super) fn record_unparseable_path(&mut self, path: &str) {
        record_unparseable_path(path, self.overlay_hash_input, self.deleted_paths);
    }

    pub(super) fn record_gitlink_file(
        &mut self,
        root: &Path,
        submodule_path: &str,
        commit: &str,
        entry: &source_gitlink::SubmodulePathEntry,
    ) -> Result<(), CodeIndexError> {
        let bytes =
            source_gitlink::submodule_entry_bytes(root, submodule_path, commit, &entry.child_path)?;
        let blob_hash = stable_content_hash(&bytes);
        self.overlay_hash_input.extend_from_slice(b"F\0");
        self.overlay_hash_input
            .extend_from_slice(entry.parent_path.as_bytes());
        self.overlay_hash_input.push(0);
        self.overlay_hash_input
            .extend_from_slice(blob_hash.as_bytes());
        self.overlay_hash_input.push(0);
        let was_deleted = self
            .deleted_paths
            .iter()
            .any(|path| path == &entry.parent_path);
        self.deleted_paths.retain(|path| path != &entry.parent_path);
        if self.previous_hashes.get(&entry.parent_path) == Some(&blob_hash) && !was_deleted {
            *self.skipped_unchanged_count += 1;
            return Ok(());
        }
        self.files_to_parse.push((entry.parent_path.clone(), bytes));

        Ok(())
    }
}

#[cfg(test)]
#[path = "recorder_tests.rs"]
mod tests;
