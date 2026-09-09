//! Assembles verified worktree-overlay snapshots and workspace entries.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::domain::{
    CodeIndexSnapshot, CodeRepositoryRegistration, CodeRepositorySelector,
    CodeWorkspaceDetectionConfig,
};

use super::super::{
    CodeIndexError, changes,
    filesystem_delta::build_filesystem_delta_snapshot,
    full_snapshot::build_full_snapshot_as_worktree_overlay,
    git::{git_bytes, resolve_ref},
    parser::parse_indexed_file,
    snapshot::{self, SnapshotBuild, SnapshotScopeFilters},
    source::{RepositorySourceKind, source_commit_is_filesystem, source_kind},
};
use super::{
    change_recording::{WorktreeChangeContext, record_worktree_change},
    overlay_plan::WorktreeOverlayPlan,
    overlay_scope::{WorktreeOverlayScope, bounded_worktree_changes},
};

pub(in crate::code::index) fn worktree_overlay_identity(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    previous_hashes: &BTreeMap<String, String>,
    base_resolved_commit_sha: Option<&str>,
) -> Result<(String, String), CodeIndexError> {
    if source_commit_is_filesystem(&selector.ref_selector)
        || base_resolved_commit_sha.is_some_and(source_commit_is_filesystem)
        || source_kind(root)?.is_filesystem()
    {
        let snapshot = build_filesystem_delta_snapshot(
            registration,
            selector,
            root,
            &selector.ref_selector,
            previous_hashes,
            base_resolved_commit_sha,
            &Default::default(),
        )?;
        return Ok((snapshot.resolved_commit_sha, snapshot.tree_hash));
    }
    let plan = plan_worktree_overlay(registration, selector, root, previous_hashes)?;
    Ok(plan.identity())
}

pub(in crate::code::index) fn build_worktree_overlay_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    previous_hashes: &BTreeMap<String, String>,
    base_resolved_commit_sha: Option<&str>,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    if source_commit_is_filesystem(&selector.ref_selector)
        || base_resolved_commit_sha.is_some_and(source_commit_is_filesystem)
        || source_kind(root)?.is_filesystem()
    {
        return build_filesystem_delta_snapshot(
            registration,
            selector,
            root,
            &selector.ref_selector,
            previous_hashes,
            base_resolved_commit_sha,
            workspace_detection,
        );
    }
    let plan = plan_worktree_overlay(registration, selector, root, previous_hashes)?;
    if plan.overlay_hash_input.is_empty() {
        return build_full_snapshot_as_worktree_overlay(
            registration,
            selector,
            root,
            &selector.ref_selector,
            &plan.commit,
            workspace_detection,
        );
    }
    let (overlay_commit, tree_hash) = plan.identity();
    let language_filters =
        snapshot::merged_filters(&registration.language_filters, &selector.language_filters);
    let workspace_entries =
        workspace_overlay_entries(previous_hashes, &plan.deleted_paths, &plan.files_to_parse);
    let python_module_origins =
        crate::code::python_imports::PythonModuleOrigins::from_authorized_paths(
            workspace_entries.iter().map(|entry| entry.path.as_str()),
            &plan.path_filters,
            &language_filters,
        );
    let reparse_python = python_module_origins
        != crate::code::python_imports::PythonModuleOrigins::from_authorized_paths(
            previous_hashes.keys().map(String::as_str),
            &plan.path_filters,
            &language_filters,
        );
    let base_commit = plan.commit.clone();
    let mut build = SnapshotBuild::new_with_scope_filters(
        registration,
        overlay_commit,
        tree_hash,
        SnapshotScopeFilters {
            path_filters: plan.path_filters.clone(),
            language_filters,
        },
        false,
        plan.changed_path_count,
        plan.skipped_unchanged_count,
    );
    build.base_resolved_commit_sha = Some(plan.commit);
    build.python_module_origins = python_module_origins;
    let deleted_paths = plan.deleted_paths;
    let files_to_parse = plan.files_to_parse;
    let mut skipped_python_paths = plan.skipped_python_paths;
    let parsed_paths = files_to_parse
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    build.deleted_paths = deleted_paths;

    build.detect_and_fill_workspaces(
        root,
        RepositorySourceKind::FileSystem,
        &workspace_entries,
        workspace_detection,
    );

    for (path, bytes) in files_to_parse {
        parse_indexed_file(&mut build, &path, &bytes)?;
    }
    if reparse_python {
        let mut total_bytes = 0usize;
        for entry in &workspace_entries {
            if !entry.path.ends_with(".py") || parsed_paths.contains(&entry.path) {
                continue;
            }
            if build.files.len() >= crate::code::index::MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS {
                return Err(CodeIndexError::InvalidInput("Python import-origin overlay reparse exceeds the bounded file budget; commit changes and run a full code index".into()));
            }
            let bytes = crate::code::source::source_bytes_after_content_verification(
                root,
                &base_commit,
                &entry.path,
                None,
            )?;
            total_bytes = total_bytes.saturating_add(bytes.len());
            if total_bytes > crate::domain::CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH {
                return Err(CodeIndexError::InvalidInput("Python import-origin overlay reparse exceeds the bounded byte budget; commit changes and run a full code index".into()));
            }
            parse_indexed_file(&mut build, &entry.path, &bytes)?;
            if skipped_python_paths.remove(&entry.path) {
                build.skipped_unchanged_count -= 1;
            }
        }
    }

    Ok(build.finish())
}

fn plan_worktree_overlay(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    previous_hashes: &BTreeMap<String, String>,
) -> Result<WorktreeOverlayPlan, CodeIndexError> {
    let commit = resolve_ref(root, &selector.ref_selector)?;
    let head_commit = resolve_ref(root, "HEAD")?;
    if commit != head_commit {
        return Err(CodeIndexError::InvalidInput(format!(
            "worktree overlay ref '{}' resolves to {}, but checked-out HEAD is {}",
            selector.ref_selector, commit, head_commit
        )));
    }
    let status = git_bytes(
        root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let changes = changes::worktree_changed_paths(&status);
    let overlay_scope = WorktreeOverlayScope::new(registration, selector, previous_hashes);
    if changes.is_empty() {
        return Ok(WorktreeOverlayPlan {
            commit,
            changed_path_count: 0,
            path_filters: overlay_scope.path_filters,
            overlay_hash_input: Vec::new(),
            deleted_paths: Vec::new(),
            files_to_parse: Vec::new(),
            skipped_unchanged_count: 0,
            skipped_python_paths: Default::default(),
        });
    }
    let changes = bounded_worktree_changes(changes, &overlay_scope)?;
    let mut overlay_hash_input = Vec::new();
    let mut deleted_paths = Vec::new();
    let mut files_to_parse = Vec::new();
    let mut skipped_unchanged_count = 0;
    let mut skipped_python_paths = std::collections::BTreeSet::new();
    let context = WorktreeChangeContext {
        root,
        commit: &commit,
        previous_hashes,
        overlay_scope: &overlay_scope,
    };
    let mut outputs = super::recording::WorktreeFileOutputs {
        overlay_hash_input: &mut overlay_hash_input,
        deleted_paths: &mut deleted_paths,
        files_to_parse: &mut files_to_parse,
        skipped_unchanged_count: &mut skipped_unchanged_count,
        skipped_python_paths: &mut skipped_python_paths,
    };
    for change in &changes {
        record_worktree_change(&context, change, &mut outputs)?;
    }

    Ok(WorktreeOverlayPlan {
        commit,
        changed_path_count: changes.len(),
        path_filters: overlay_scope.path_filters,
        overlay_hash_input,
        deleted_paths,
        files_to_parse,
        skipped_unchanged_count,
        skipped_python_paths,
    })
}

fn workspace_overlay_entries(
    previous_hashes: &BTreeMap<String, String>,
    deleted_paths: &[String],
    files_to_parse: &[(String, Vec<u8>)],
) -> Vec<changes::GitTreeEntry> {
    let deleted = deleted_paths.iter().collect::<BTreeSet<_>>();
    let mut entries = previous_hashes
        .keys()
        .filter(|path| !deleted.contains(path))
        .map(|path| (path.clone(), 0usize))
        .collect::<BTreeMap<_, _>>();
    for (path, bytes) in files_to_parse {
        entries.insert(path.clone(), bytes.len());
    }
    entries
        .into_iter()
        .map(|(path, byte_count)| changes::GitTreeEntry { path, byte_count })
        .collect()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
