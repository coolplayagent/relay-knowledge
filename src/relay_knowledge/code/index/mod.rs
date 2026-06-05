//! Code index snapshot orchestration and impact seed discovery.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

mod deleted_symbols;
pub(in crate::code) mod filesystem_delta;
#[path = "full.rs"]
mod full_snapshot;
mod impact_paths;
mod incremental;
pub(in crate::code) mod plan;
pub(in crate::code) mod snapshot;
mod worktree_overlay;

use crate::domain::{
    CodeFileFingerprint, CodeIndexMode, CodeIndexResourceBudget, CodeIndexSnapshot,
    CodeRepositoryRegistration, CodeRepositorySelector,
};

#[cfg(test)]
use crate::code::source::changes::GitChange;
use crate::code::{
    CodeIndexError, identity, ids, parser,
    parser::parse_indexed_file,
    source::{
        self, changes,
        changes::{TrackedEntryScope, diff_changes},
        git, gitlink as source_gitlink,
        layout::{self as scope, scoped_source_snapshot_for_filters},
        resolution::resolve_repository_ref_with_filters,
        source_commit_is_filesystem, source_kind,
    },
};
pub use deleted_symbols::deleted_symbol_names_for_diff;
pub(crate) use filesystem_delta::changed_paths_for_filesystem_diff;
use full_snapshot::build_full_snapshot;
#[cfg(test)]
pub(crate) use full_snapshot::mutate_next_filesystem_full_snapshot_read;
use incremental::build_incremental_snapshot;
pub use plan::{CodeIndexPlan, prepare_full_index_plan};
use worktree_overlay::build_worktree_overlay_snapshot;

pub(in crate::code) const MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS: usize =
    CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH;

/// Builds a code index snapshot from a clean Git commit or incremental diff.
pub fn build_index_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    mode: CodeIndexMode,
    previous_hashes: Vec<CodeFileFingerprint>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    build_index_snapshot_with_base_commit(registration, selector, mode, previous_hashes, None)
}

pub(crate) fn build_index_snapshot_with_base_commit(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    mode: CodeIndexMode,
    previous_hashes: Vec<CodeFileFingerprint>,
    base_resolved_commit_sha: Option<String>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let previous_hashes = previous_hashes
        .into_iter()
        .map(|fingerprint| (fingerprint.path, fingerprint.blob_hash))
        .collect::<BTreeMap<_, _>>();

    match mode {
        CodeIndexMode::Full => build_full_snapshot(registration, selector, &root),
        CodeIndexMode::Incremental { base_ref, head_ref } => build_incremental_snapshot(
            registration,
            selector,
            &root,
            &base_ref,
            &head_ref,
            &previous_hashes,
            base_resolved_commit_sha.as_deref(),
        ),
        CodeIndexMode::WorktreeOverlay => build_worktree_overlay_snapshot(
            registration,
            selector,
            &root,
            &previous_hashes,
            base_resolved_commit_sha.as_deref(),
        ),
    }
}

pub fn changed_paths_for_diff(
    root_path: impl AsRef<Path>,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<String>, CodeIndexError> {
    changed_paths_for_diff_with_filters(root_path, base_ref, head_ref, &[], &[])
}

pub fn changed_paths_for_diff_with_path_filters(
    root_path: impl AsRef<Path>,
    base_ref: &str,
    head_ref: &str,
    path_filters: &[String],
) -> Result<Vec<String>, CodeIndexError> {
    changed_paths_for_diff_with_filters(root_path, base_ref, head_ref, path_filters, &[])
}

pub fn changed_paths_for_diff_with_filters(
    root_path: impl AsRef<Path>,
    base_ref: &str,
    head_ref: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<Vec<String>, CodeIndexError> {
    if source_commit_is_filesystem(base_ref) || source_commit_is_filesystem(head_ref) {
        if base_ref == head_ref {
            return Ok(Vec::new());
        }
        let base_commit = resolve_repository_ref_with_filters(
            root_path.as_ref(),
            base_ref,
            path_filters,
            language_filters,
        )?;
        let head_commit = resolve_repository_ref_with_filters(
            root_path.as_ref(),
            head_ref,
            path_filters,
            language_filters,
        )?;
        if base_commit == head_commit {
            return Ok(Vec::new());
        }
        let snapshot = scoped_source_snapshot_for_filters(
            root_path.as_ref(),
            head_ref,
            path_filters,
            language_filters,
        )?;
        return Ok(snapshot
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect());
    }
    if source_kind(root_path.as_ref())?.is_filesystem() {
        if base_ref == head_ref {
            return Ok(Vec::new());
        }
        let snapshot = scoped_source_snapshot_for_filters(
            root_path.as_ref(),
            head_ref,
            path_filters,
            language_filters,
        )?;
        return Ok(snapshot
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect());
    }
    let changes = diff_changes(root_path.as_ref(), base_ref, head_ref)?;

    impact_paths::paths_from_changes_with_gitlinks(
        root_path.as_ref(),
        base_ref,
        head_ref,
        changes,
        path_filters,
        language_filters,
    )
}

#[cfg(test)]
pub(in crate::code) fn impact_paths_from_changes(changes: Vec<GitChange>) -> Vec<String> {
    let mut paths = Vec::new();
    for change in changes {
        match change {
            GitChange::AddedOrModified { path }
            | GitChange::Deleted { path }
            | GitChange::TypeChanged { path } => paths.push(path),
            GitChange::Renamed { old_path, new_path } => {
                paths.push(old_path);
                paths.push(new_path);
            }
            GitChange::Copied { new_path, .. } => paths.push(new_path),
        }
    }
    paths.sort();
    paths.dedup();

    paths
}

pub(crate) fn repository_uses_filesystem_source(
    root_path: impl AsRef<Path>,
) -> Result<bool, CodeIndexError> {
    Ok(source_kind(root_path.as_ref())?.is_filesystem())
}

pub(in crate::code::index) fn tracked_entry_scope_for_selector(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> TrackedEntryScope {
    match scope::intersect_path_filters(&registration.path_filters, &selector.path_filters) {
        Some(filters) => TrackedEntryScope::from_path_filters(filters.iter()),
        None => TrackedEntryScope::empty(),
    }
}
