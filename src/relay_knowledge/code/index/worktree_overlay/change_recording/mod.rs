//! Routes bounded worktree changes into parse and deletion queues.

use std::{collections::BTreeMap, fs, path::Path};

use crate::code::{CodeIndexError, source::changes};

use super::gitlinks::recorder::WorktreeOverlayRecorder;
use super::{
    directories::{
        contains_git_metadata, worktree_directory_files, worktree_directory_is_expandable,
    },
    gitlinks::{
        record_deleted_gitlink_overlay, record_staged_gitlink_overlay,
        record_unstaged_gitlink_overlay,
    },
    overlay_scope::WorktreeOverlayScope,
    recording::{
        WorktreeFileOutputs, record_deleted_path, record_file_as, record_unparseable_path,
    },
};

pub(super) struct WorktreeChangeContext<'a, 'scope> {
    pub(super) root: &'a Path,
    pub(super) commit: &'a str,
    pub(super) previous_hashes: &'a BTreeMap<String, String>,
    pub(super) overlay_scope: &'a WorktreeOverlayScope<'scope>,
}

pub(super) fn record_worktree_change(
    context: &WorktreeChangeContext<'_, '_>,
    change: &changes::WorktreePathChange,
    outputs: &mut WorktreeFileOutputs<'_>,
) -> Result<(), CodeIndexError> {
    if let Some(deleted_path) = &change.deleted_source {
        let deleted_gitlink = if context.overlay_scope.overlaps(deleted_path) {
            let mut recorder = WorktreeOverlayRecorder {
                scope: context.overlay_scope,
                previous_hashes: context.previous_hashes,
                overlay_hash_input: &mut *outputs.overlay_hash_input,
                deleted_paths: &mut *outputs.deleted_paths,
                files_to_parse: &mut *outputs.files_to_parse,
                skipped_unchanged_count: &mut *outputs.skipped_unchanged_count,
            };
            record_deleted_gitlink_overlay(
                context.root,
                context.commit,
                deleted_path,
                &mut recorder,
            )?
        } else {
            false
        };
        if !deleted_gitlink && context.overlay_scope.selected(deleted_path) {
            record_deleted_path(
                deleted_path,
                &mut *outputs.overlay_hash_input,
                &mut *outputs.deleted_paths,
            );
        }
    }
    let path = &change.path;
    if !context.overlay_scope.overlaps(path) {
        return Ok(());
    }
    if change.is_untracked() && !context.overlay_scope.untracked_selected(path) {
        return Ok(());
    }
    {
        let mut recorder = WorktreeOverlayRecorder {
            scope: context.overlay_scope,
            previous_hashes: context.previous_hashes,
            overlay_hash_input: &mut *outputs.overlay_hash_input,
            deleted_paths: &mut *outputs.deleted_paths,
            files_to_parse: &mut *outputs.files_to_parse,
            skipped_unchanged_count: &mut *outputs.skipped_unchanged_count,
        };
        if record_staged_gitlink_overlay(change, context.root, context.commit, &mut recorder)? {
            return Ok(());
        }
    }
    record_worktree_path(context, change, outputs)
}

fn record_worktree_path(
    context: &WorktreeChangeContext<'_, '_>,
    change: &changes::WorktreePathChange,
    outputs: &mut WorktreeFileOutputs<'_>,
) -> Result<(), CodeIndexError> {
    let path = &change.path;
    let full_path = context.root.join(path);
    let metadata = match fs::symlink_metadata(&full_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut recorder = WorktreeOverlayRecorder {
                scope: context.overlay_scope,
                previous_hashes: context.previous_hashes,
                overlay_hash_input: &mut *outputs.overlay_hash_input,
                deleted_paths: &mut *outputs.deleted_paths,
                files_to_parse: &mut *outputs.files_to_parse,
                skipped_unchanged_count: &mut *outputs.skipped_unchanged_count,
            };
            if record_deleted_gitlink_overlay(context.root, context.commit, path, &mut recorder)? {
                return Ok(());
            } else if context.overlay_scope.selected(path) {
                record_deleted_path(
                    path,
                    &mut *outputs.overlay_hash_input,
                    &mut *outputs.deleted_paths,
                );
            }
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        if context.overlay_scope.selected(path) {
            record_unparseable_path(
                path,
                &mut *outputs.overlay_hash_input,
                &mut *outputs.deleted_paths,
            );
        }
        return Ok(());
    }
    if file_type.is_dir() {
        return record_worktree_directory(context, change, outputs);
    }
    if !file_type.is_file() {
        if context.overlay_scope.selected(path) {
            record_unparseable_path(
                path,
                &mut *outputs.overlay_hash_input,
                &mut *outputs.deleted_paths,
            );
        }
        return Ok(());
    }
    if context.overlay_scope.selected(path) {
        record_file_as(context.root, path, path, context.previous_hashes, outputs)?;
    }

    Ok(())
}

fn record_worktree_directory(
    context: &WorktreeChangeContext<'_, '_>,
    change: &changes::WorktreePathChange,
    outputs: &mut WorktreeFileOutputs<'_>,
) -> Result<(), CodeIndexError> {
    let path = &change.path;
    if contains_git_metadata(context.root, Path::new(path))? {
        let mut recorder = WorktreeOverlayRecorder {
            scope: context.overlay_scope,
            previous_hashes: context.previous_hashes,
            overlay_hash_input: &mut *outputs.overlay_hash_input,
            deleted_paths: &mut *outputs.deleted_paths,
            files_to_parse: &mut *outputs.files_to_parse,
            skipped_unchanged_count: &mut *outputs.skipped_unchanged_count,
        };
        record_unstaged_gitlink_overlay(context.root, context.commit, path, &mut recorder)?;
        return Ok(());
    }
    if !change.is_untracked() || !worktree_directory_is_expandable(context.root, path)? {
        if context.overlay_scope.selected(path) {
            record_unparseable_path(
                path,
                &mut *outputs.overlay_hash_input,
                &mut *outputs.deleted_paths,
            );
        }
        return Ok(());
    }
    for nested_path in worktree_directory_files(context.root, path)? {
        if context.overlay_scope.untracked_selected(&nested_path) {
            record_file_as(
                context.root,
                &nested_path,
                &nested_path,
                context.previous_hashes,
                outputs,
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
