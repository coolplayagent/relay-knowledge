//! Bounded local reads and identity-safe restart after a newly observed I/O failure.

use super::*;
use crate::code::source::{
    path_io::SkippedSourcePath, source_snapshot_bytes, tree_hash_with_skipped,
};
use crate::domain::{CodePathIoOperation, CodePathKind};

const MAX_SOURCE_REPLANS_PER_ATTEMPT: usize = 2;

impl CodeIndexPlan {
    pub(super) fn parse_local_group(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> Result<Vec<PendingParsedFile>, CodeIndexError> {
        // Losing the root is a task-wide failure, even if child reads report NotFound.
        std::fs::read_dir(&self.root)?;
        let mut parsed = BTreeMap::new();
        let mut readable_paths = Vec::new();
        let mut blobs = Vec::new();
        let paths = self.paths[range].to_vec();
        for entry in &paths {
            if !self.skipped_paths.contains_key(&entry.path) {
                match source_snapshot_bytes(&self.root, self.source_kind, &self.commit, &entry.path)
                {
                    Ok(bytes) => {
                        ensure_filesystem_blobs_match_content_hashes(
                            &self.commit,
                            std::slice::from_ref(&entry.path),
                            std::slice::from_ref(&bytes),
                            &self.filesystem_path_hashes,
                        )?;
                        readable_paths.push(entry.path.clone());
                        blobs.push(bytes);
                        continue;
                    }
                    Err(CodeIndexError::Io(error)) => {
                        std::fs::read_dir(&self.root)?;
                        let skipped = SkippedSourcePath::from_error(
                            &entry.path,
                            CodePathKind::File,
                            CodePathIoOperation::Read,
                            error,
                        )?;
                        self.skipped_paths.insert(entry.path.clone(), skipped);
                        self.needs_source_replan = true;
                    }
                    Err(error) => return Err(error),
                }
            }
            let mut build = SnapshotBuild::new_with_scope_filters(
                &self.registration,
                self.commit.clone(),
                self.tree_hash.clone(),
                SnapshotScopeFilters {
                    path_filters: self.path_filters.clone(),
                    language_filters: self.language_filters.clone(),
                },
                true,
                self.paths.len(),
                0,
            );
            build.bind_verified_source_scope(&self.source_scope)?;
            build.diagnostics.push(
                self.skipped_paths[&entry.path]
                    .diagnostic(&build.repository_id, &build.source_scope),
            );
            parsed.insert(
                entry.path.clone(),
                PendingParsedFile {
                    parsed_byte_count: 0,
                    build,
                },
            );
        }
        let builds = parse_fetched_files(self, &readable_paths, &blobs)?;
        for ((path, bytes), build) in readable_paths.into_iter().zip(blobs).zip(builds) {
            parsed.insert(
                path,
                PendingParsedFile {
                    parsed_byte_count: bytes.len(),
                    build,
                },
            );
        }
        paths
            .into_iter()
            .map(|entry| {
                parsed
                    .remove(&entry.path)
                    .ok_or_else(|| invalid_checkpoint("missing local source outcome"))
            })
            .collect()
    }

    /// Provisional batches remain unpublished. Replay under the observed partial identity,
    /// never rereading a path that already failed during this attempt.
    pub(crate) fn replan_after_source_failures(mut self) -> Result<Option<Self>, CodeIndexError> {
        if !self.needs_source_replan {
            return Ok(None);
        }
        if self.source_replan_count >= MAX_SOURCE_REPLANS_PER_ATTEMPT {
            return Err(CodeIndexError::InvalidInput(
                "local source keeps changing beyond the bounded snapshot replan budget".into(),
            ));
        }
        self.source_replan_count += 1;
        self.filesystem_path_hashes
            .retain(|path, _| !self.skipped_paths.contains_key(path));
        let skipped = self.skipped_paths.values().cloned().collect::<Vec<_>>();
        let tree = tree_hash_with_skipped(&self.filesystem_path_hashes, &skipped);
        if let Some(pin) = &self.filesystem_ref_pin {
            crate::code::source::ensure_filesystem_snapshot_matches_ref(pin, &tree)?;
        }
        self.tree_hash = tree;
        self.commit = self.tree_hash.clone();
        self.source_scope = crate::domain::code_snapshot_scope_id_with_workspace_detection(
            &self.registration.repository_id,
            &self.tree_hash,
            &self.path_filters,
            &self.language_filters,
            &self.workspace_detection,
        );
        let entries = self
            .paths
            .iter()
            .filter(|entry| !self.skipped_paths.contains_key(&entry.path))
            .cloned()
            .collect::<Vec<_>>();
        self.workspaces = detect_workspaces_for_source_snapshot(
            &self.root,
            self.source_kind,
            &self.commit,
            &entries,
            &self.path_filters,
            &self.workspace_detection,
        );
        self.cursor = 0;
        self.next_batch_index = 1;
        self.parsed_overflow.clear();
        self.needs_source_replan = false;
        Ok(Some(self))
    }
}

#[cfg(test)]
#[path = "local_source_tests.rs"]
mod tests;
