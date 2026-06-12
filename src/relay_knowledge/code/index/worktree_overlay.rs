use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use crate::domain::{
    CodeIndexSnapshot, CodeRepositoryRegistration, CodeRepositorySelector,
    CodeWorkspaceDetectionConfig,
};

use super::{
    CodeIndexError, MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS, changes,
    filesystem_delta::build_filesystem_delta_snapshot,
    full_snapshot::build_full_snapshot_as_worktree_overlay,
    git::{git_bytes, resolve_ref},
    ids::stable_content_hash,
    parser::parse_indexed_file,
    scope,
    snapshot::{self, SnapshotBuild, SnapshotScopeFilters},
    source::{RepositorySourceKind, source_commit_is_filesystem, source_kind},
    source_gitlink,
};

#[path = "worktree_overlay_dirs.rs"]
mod dirs;
#[path = "worktree_overlay_git.rs"]
mod git_overlay;
#[path = "worktree_overlay_plan.rs"]
mod overlay_plan;
#[path = "worktree_overlay_scope.rs"]
mod overlay_scope;
#[path = "worktree_overlay_untracked.rs"]
mod worktree_overlay_untracked;

use dirs::{worktree_directory_files, worktree_directory_is_expandable};
use git_overlay::{
    StagedPathKind, base_path_exists, contains_git_metadata, staged_path_kind,
    submodule_worktree_head, submodule_worktree_parent_path,
};
use overlay_plan::WorktreeOverlayPlan;
use overlay_scope::{WorktreeOverlayScope, bounded_worktree_changes};

pub(super) fn worktree_overlay_identity(
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

pub(super) fn build_worktree_overlay_snapshot(
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
    let deleted_paths = plan.deleted_paths;
    let files_to_parse = plan.files_to_parse;
    let workspace_entries =
        workspace_overlay_entries(previous_hashes, &deleted_paths, &files_to_parse);
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
        });
    }
    let changes = bounded_worktree_changes(changes, &overlay_scope)?;
    let mut overlay_hash_input = Vec::new();
    let mut deleted_paths = Vec::new();
    let mut files_to_parse = Vec::new();
    let mut skipped_unchanged_count = 0;
    let context = WorktreeChangeContext {
        root,
        commit: &commit,
        previous_hashes,
        overlay_scope: &overlay_scope,
    };
    let mut outputs = WorktreeFileOutputs {
        overlay_hash_input: &mut overlay_hash_input,
        deleted_paths: &mut deleted_paths,
        files_to_parse: &mut files_to_parse,
        skipped_unchanged_count: &mut skipped_unchanged_count,
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
    })
}

struct WorktreeChangeContext<'a, 'scope> {
    root: &'a Path,
    commit: &'a str,
    previous_hashes: &'a BTreeMap<String, String>,
    overlay_scope: &'a WorktreeOverlayScope<'scope>,
}

fn record_worktree_change(
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
            record_worktree_deleted_path(
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
                record_worktree_deleted_path(
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
            record_unparseable_worktree_path(
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
            record_unparseable_worktree_path(
                path,
                &mut *outputs.overlay_hash_input,
                &mut *outputs.deleted_paths,
            );
        }
        return Ok(());
    }
    if context.overlay_scope.selected(path) {
        record_worktree_file(
            context.root,
            path,
            context.previous_hashes,
            &mut *outputs.overlay_hash_input,
            &mut *outputs.deleted_paths,
            &mut *outputs.files_to_parse,
            &mut *outputs.skipped_unchanged_count,
        )?;
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
            record_unparseable_worktree_path(
                path,
                &mut *outputs.overlay_hash_input,
                &mut *outputs.deleted_paths,
            );
        }
        return Ok(());
    }
    for nested_path in worktree_directory_files(context.root, path)? {
        if context.overlay_scope.untracked_selected(&nested_path) {
            record_worktree_file(
                context.root,
                &nested_path,
                context.previous_hashes,
                &mut *outputs.overlay_hash_input,
                &mut *outputs.deleted_paths,
                &mut *outputs.files_to_parse,
                &mut *outputs.skipped_unchanged_count,
            )?;
        }
    }

    Ok(())
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

fn record_worktree_status_marker(path: &str, overlay_hash_input: &mut Vec<u8>) {
    overlay_hash_input.extend_from_slice(b"S\0");
    overlay_hash_input.extend_from_slice(path.as_bytes());
    overlay_hash_input.push(0);
}

fn record_worktree_deleted_path(
    path: &str,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
) {
    overlay_hash_input.extend_from_slice(b"D\0");
    overlay_hash_input.extend_from_slice(path.as_bytes());
    overlay_hash_input.push(0);
    deleted_paths.push(path.to_owned());
}

fn record_unparseable_worktree_path(
    path: &str,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
) {
    record_worktree_status_marker(path, overlay_hash_input);
    record_worktree_deleted_path(path, overlay_hash_input, deleted_paths);
}

fn record_previous_gitlink_child_deletions(
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
        record_worktree_deleted_path(path, overlay_hash_input, deleted_paths);
    }

    Ok(!paths.is_empty())
}

fn record_worktree_file(
    root: &Path,
    path: &str,
    previous_hashes: &BTreeMap<String, String>,
    overlay_hash_input: &mut Vec<u8>,
    deleted_paths: &mut Vec<String>,
    files_to_parse: &mut Vec<(String, Vec<u8>)>,
    skipped_unchanged_count: &mut usize,
) -> Result<(), CodeIndexError> {
    let mut outputs = WorktreeFileOutputs {
        overlay_hash_input,
        deleted_paths,
        files_to_parse,
        skipped_unchanged_count,
    };
    record_worktree_file_as(root, path, path, previous_hashes, &mut outputs)
}

struct WorktreeFileOutputs<'a> {
    overlay_hash_input: &'a mut Vec<u8>,
    deleted_paths: &'a mut Vec<String>,
    files_to_parse: &'a mut Vec<(String, Vec<u8>)>,
    skipped_unchanged_count: &'a mut usize,
}

fn record_worktree_file_as(
    root: &Path,
    source_path: &str,
    indexed_path: &str,
    previous_hashes: &BTreeMap<String, String>,
    outputs: &mut WorktreeFileOutputs<'_>,
) -> Result<(), CodeIndexError> {
    let bytes = fs::read(root.join(source_path))?;
    let blob_hash = stable_content_hash(&bytes);
    outputs.overlay_hash_input.extend_from_slice(b"F\0");
    outputs
        .overlay_hash_input
        .extend_from_slice(indexed_path.as_bytes());
    outputs.overlay_hash_input.push(0);
    outputs
        .overlay_hash_input
        .extend_from_slice(blob_hash.as_bytes());
    outputs.overlay_hash_input.push(0);
    let was_deleted = outputs
        .deleted_paths
        .iter()
        .any(|path| path == indexed_path);
    outputs.deleted_paths.retain(|path| path != indexed_path);
    if previous_hashes.get(indexed_path) == Some(&blob_hash) && !was_deleted {
        *outputs.skipped_unchanged_count += 1;
        return Ok(());
    }
    outputs
        .files_to_parse
        .retain(|(path, _)| path != indexed_path);
    outputs
        .files_to_parse
        .push((indexed_path.to_owned(), bytes));

    Ok(())
}

fn record_deleted_gitlink_overlay(
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
            record_worktree_status_marker(path, recorder.overlay_hash_input);
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

struct WorktreeOverlayRecorder<'a, 'scope> {
    scope: &'a WorktreeOverlayScope<'scope>,
    previous_hashes: &'a BTreeMap<String, String>,
    overlay_hash_input: &'a mut Vec<u8>,
    deleted_paths: &'a mut Vec<String>,
    files_to_parse: &'a mut Vec<(String, Vec<u8>)>,
    skipped_unchanged_count: &'a mut usize,
}

impl WorktreeOverlayRecorder<'_, '_> {
    fn path_is_selected(&self, path: &str) -> bool {
        self.scope.selected(path)
    }

    fn path_scope_overlaps(&self, path: &str) -> bool {
        self.scope.overlaps(path)
    }

    fn untracked_path_is_selected(&self, path: &str) -> bool {
        self.scope.untracked_selected(path)
    }

    fn record_deleted_path(&mut self, path: &str) {
        record_worktree_deleted_path(path, self.overlay_hash_input, self.deleted_paths);
    }

    fn record_unparseable_path(&mut self, path: &str) {
        record_unparseable_worktree_path(path, self.overlay_hash_input, self.deleted_paths);
    }

    fn record_gitlink_file(
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

fn record_staged_gitlink_overlay(
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
        record_worktree_status_marker(path, recorder.overlay_hash_input);
    }
    for entry in staged_entries {
        recorder.record_gitlink_file(root, path, gitlink_commit, &entry)?;
    }

    Ok(())
}

fn record_unstaged_gitlink_overlay(
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
        record_worktree_status_marker(path, recorder.overlay_hash_input);
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
        return record_worktree_file_as(
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
                record_worktree_file_as(
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
        record_worktree_status_marker(parent_path, recorder.overlay_hash_input);
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
        record_worktree_status_marker(path, recorder.overlay_hash_input);
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
        record_worktree_status_marker(path, recorder.overlay_hash_input);
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
