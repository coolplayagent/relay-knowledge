//! Builds verified incremental snapshots from bounded changed paths.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::domain::{
    CodeIndexResourceBudget, CodeIndexSnapshot, CodePathTombstone, CodeRepositoryRegistration,
    CodeRepositorySelector, CodeWorkspaceDetectionConfig,
};

use super::{
    MAX_INCREMENTAL_CHANGED_PATHS, MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
    filesystem_delta::build_filesystem_delta_snapshot, tracked_entry_scope_for_selector,
};
use crate::code::{
    CodeIndexError,
    changes::{
        self, GitChange, diff_changes, tracked_entries_state_with_scope, tracked_entries_with_scope,
    },
    git::{resolve_ref, resolve_tree},
    ids::stable_content_hash,
    parser::parse_indexed_file,
    scope::{
        self, discover_source_layout, effective_index_path_filters_for_layouts,
        path_is_selected_with_layout, path_scope_overlaps,
    },
    snapshot::{self, SnapshotBuild, SnapshotScopeFilters},
    source::{
        RepositorySourceKind, git_tree_hash_with_submodules, gitlink as source_gitlink,
        gitlink::paths as source_gitlink_paths, source_batch_bytes_after_content_verification,
        source_bytes_after_content_verification, source_commit_is_filesystem, source_kind,
    },
};

pub(super) struct IncrementalSnapshotRequest<'a> {
    pub base_ref: &'a str,
    pub head_ref: &'a str,
    pub previous_hashes: &'a BTreeMap<String, String>,
    pub base_resolved_commit_sha: Option<&'a str>,
    pub workspace_detection: &'a CodeWorkspaceDetectionConfig,
}

pub(super) fn build_incremental_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    request: IncrementalSnapshotRequest<'_>,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    if source_commit_is_filesystem(request.base_ref)
        || source_commit_is_filesystem(request.head_ref)
        || request
            .base_resolved_commit_sha
            .is_some_and(source_commit_is_filesystem)
        || source_kind(root)?.is_filesystem()
    {
        return build_filesystem_delta_snapshot(
            registration,
            selector,
            root,
            request.head_ref,
            request.previous_hashes,
            request.base_resolved_commit_sha,
            request.workspace_detection,
        );
    }
    let base_commit = resolve_ref(root, request.base_ref)?;
    let commit = resolve_ref(root, request.head_ref)?;
    let parent_tree_hash = resolve_tree(root, &commit)?;
    let changes = diff_changes(root, &base_commit, &commit)?;
    validate_changed_path_budget(changes.len())?;
    let entry_scope = tracked_entry_scope_for_selector(registration, selector);
    let base_entries = tracked_entries_with_scope(root, &base_commit, &entry_scope)?;
    let base_source_layout = discover_source_layout(&base_entries);
    let previous_entries = request
        .previous_hashes
        .keys()
        .map(|path| changes::GitTreeEntry {
            path: path.clone(),
            byte_count: 0,
        })
        .collect::<Vec<_>>();
    let previous_source_layout = discover_source_layout(&previous_entries);
    let head_state = tracked_entries_state_with_scope(root, &commit, &entry_scope)?;
    let tree_hash = git_tree_hash_with_submodules(&parent_tree_hash, &head_state.submodule_states);
    let head_entries = head_state.entries;
    let source_layout = discover_source_layout(&head_entries);
    let path_filters = effective_index_path_filters_for_layouts(
        registration,
        selector,
        &[&source_layout, &base_source_layout, &previous_source_layout],
    );
    let effective_path_filters = path_filters.clone();
    let language_filters =
        snapshot::merged_filters(&registration.language_filters, &selector.language_filters);
    let prefetched_bytes = prefetch_changed_path_bytes(ChangedPathPrefetchRequest {
        registration,
        selector,
        root,
        commit: &commit,
        changes: &changes,
        head_entries: &head_entries,
        source_layout: &source_layout,
        previous_source_layout: &previous_source_layout,
    })?;
    let mut build = SnapshotBuild::new_with_scope_filters(
        registration,
        commit,
        tree_hash,
        SnapshotScopeFilters {
            path_filters,
            language_filters,
        },
        false,
        changes.len(),
        0,
    );
    build.base_resolved_commit_sha = Some(base_commit.clone());

    build.detect_and_fill_workspaces(
        root,
        RepositorySourceKind::Git,
        &head_entries,
        request.workspace_detection,
    );

    let parse_context = ChangedPathParseContext {
        registration,
        selector,
        root,
        base_commit: &base_commit,
        previous_hashes: request.previous_hashes,
        source_layout: &source_layout,
        previous_source_layout: &previous_source_layout,
        effective_path_filters: &effective_path_filters,
        prefetched_bytes: &prefetched_bytes,
    };

    for change in changes {
        match change {
            GitChange::Deleted { path } => {
                if delete_expanded_gitlink_paths(
                    &mut build,
                    &parse_context,
                    &base_entries,
                    &base_source_layout,
                    &path,
                )? {
                    continue;
                }
                if path_is_selected_with_layout(&path, registration, selector, &base_source_layout)
                {
                    build.deleted_paths.push(path);
                }
            }
            GitChange::Renamed { old_path, new_path } => {
                if delete_expanded_gitlink_paths(
                    &mut build,
                    &parse_context,
                    &base_entries,
                    &base_source_layout,
                    &old_path,
                )? {
                    if !parse_expanded_gitlink_paths(
                        &mut build,
                        &parse_context,
                        &head_entries,
                        &new_path,
                    )? {
                        parse_changed_path(&mut build, &parse_context, &new_path)?;
                    }
                    continue;
                }
                if path_is_selected_with_layout(
                    &old_path,
                    registration,
                    selector,
                    &base_source_layout,
                ) {
                    build.deleted_paths.push(old_path.clone());
                    build.tombstones.push(CodePathTombstone {
                        repository_id: registration.repository_id.clone(),
                        source_scope: build.source_scope.clone(),
                        old_path,
                        new_path: Some(new_path.clone()),
                        base_ref: request.base_ref.to_owned(),
                        head_ref: request.head_ref.to_owned(),
                    });
                }
                if !parse_expanded_gitlink_paths(
                    &mut build,
                    &parse_context,
                    &head_entries,
                    &new_path,
                )? {
                    parse_changed_path(&mut build, &parse_context, &new_path)?;
                }
            }
            GitChange::Copied { old_path, new_path } => {
                if path_is_selected_with_layout(&new_path, registration, selector, &source_layout) {
                    build.tombstones.push(CodePathTombstone {
                        repository_id: registration.repository_id.clone(),
                        source_scope: build.source_scope.clone(),
                        old_path,
                        new_path: Some(new_path.clone()),
                        base_ref: request.base_ref.to_owned(),
                        head_ref: request.head_ref.to_owned(),
                    });
                }
                if !parse_expanded_gitlink_paths(
                    &mut build,
                    &parse_context,
                    &head_entries,
                    &new_path,
                )? {
                    parse_changed_path(&mut build, &parse_context, &new_path)?;
                }
            }
            GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => {
                if !parse_expanded_gitlink_change(
                    &mut build,
                    &parse_context,
                    &base_source_layout,
                    &path,
                )? {
                    parse_changed_path(&mut build, &parse_context, &path)?;
                }
            }
        }
    }

    Ok(build.finish())
}

fn validate_changed_path_budget(changed_path_count: usize) -> Result<(), CodeIndexError> {
    if changed_path_count > MAX_INCREMENTAL_CHANGED_PATHS {
        return Err(CodeIndexError::InvalidInput(format!(
            "incremental update changes {changed_path_count} paths, exceeding the bounded limit of {MAX_INCREMENTAL_CHANGED_PATHS}; run a full code index"
        )));
    }
    Ok(())
}

fn parse_expanded_gitlink_change(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    base_source_layout: &scope::SourceLayoutDiscovery,
    path: &str,
) -> Result<bool, CodeIndexError> {
    if !gitlink_scope_overlaps(path, context) {
        return Ok(false);
    }
    let include_expanded_path = |path: &str| {
        path_is_selected_with_layout(
            path,
            context.registration,
            context.selector,
            base_source_layout,
        ) || path_is_selected_with_layout(
            path,
            context.registration,
            context.selector,
            context.source_layout,
        ) || path_is_selected_with_layout(
            path,
            context.registration,
            context.selector,
            context.previous_source_layout,
        )
    };
    let expanded_scope_overlaps = |path: &str| gitlink_scope_overlaps(path, context);
    let child_filters = |path: &str| {
        scope::submodule_child_scope_filters_from_filters(path, context.effective_path_filters)
    };
    let Some(expansion) = source_gitlink::changed_gitlink_path_expansion(
        context.root,
        path,
        context.base_commit,
        &build.commit,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
        &source_gitlink::GitlinkPathSelector::new_with_child_filters(
            &include_expanded_path,
            &expanded_scope_overlaps,
            &child_filters,
        ),
    )?
    else {
        return Ok(false);
    };
    let base_paths_are_empty = expansion.base_paths.is_empty();
    for deleted_path in expansion.base_paths.difference(&expansion.head_paths) {
        if path_is_selected_with_layout(
            deleted_path,
            context.registration,
            context.selector,
            base_source_layout,
        ) {
            build.deleted_paths.push(deleted_path.clone());
        }
    }
    if expansion.base_is_gitlink && base_paths_are_empty {
        delete_previous_paths_under_except(
            build,
            context,
            base_source_layout,
            path,
            &expansion.head_paths,
        )?;
    }
    if !expansion.base_is_gitlink
        && path_is_selected_with_layout(
            path,
            context.registration,
            context.selector,
            base_source_layout,
        )
    {
        build.deleted_paths.push(path.to_owned());
    }
    for head_path in expansion.head_paths {
        parse_changed_path(build, context, &head_path)?;
    }

    Ok(expansion.head_is_gitlink)
}

fn parse_expanded_gitlink_paths(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    entries: &[changes::GitTreeEntry],
    path: &str,
) -> Result<bool, CodeIndexError> {
    let include_expanded_path = |path: &str| {
        path_is_selected_with_layout(
            path,
            context.registration,
            context.selector,
            context.source_layout,
        )
    };
    let paths = source_gitlink_paths::bounded_expanded_paths_under_with_selector(
        entries,
        path,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
        &source_gitlink::GitlinkPathSelector::new(&include_expanded_path, &include_expanded_path),
    )?;
    let expanded = !source_gitlink_paths::expanded_paths_under(entries, path).is_empty()
        || source_gitlink::gitlink_commit_at_tree(context.root, &build.commit, path)?.is_some();
    if paths.is_empty() {
        return Ok(expanded);
    }
    for path in paths {
        parse_changed_path(build, context, &path)?;
    }

    Ok(true)
}

fn delete_expanded_gitlink_paths(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    entries: &[changes::GitTreeEntry],
    source_layout: &scope::SourceLayoutDiscovery,
    path: &str,
) -> Result<bool, CodeIndexError> {
    let include_expanded_path = |path: &str| {
        path_is_selected_with_layout(path, context.registration, context.selector, source_layout)
    };
    let paths = source_gitlink_paths::bounded_expanded_paths_under_with_selector(
        entries,
        path,
        MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS,
        &source_gitlink::GitlinkPathSelector::new(&include_expanded_path, &include_expanded_path),
    )?;
    let expanded = !source_gitlink_paths::expanded_paths_under(entries, path).is_empty()
        || source_gitlink::gitlink_commit_at_tree(context.root, context.base_commit, path)?
            .is_some();
    if paths.is_empty() {
        if delete_previous_paths_under(build, context, source_layout, path)? {
            return Ok(true);
        }
        return Ok(expanded);
    }
    build.deleted_paths.extend(paths);

    Ok(true)
}

fn delete_previous_paths_under(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    source_layout: &scope::SourceLayoutDiscovery,
    path: &str,
) -> Result<bool, CodeIndexError> {
    delete_previous_paths_under_except(build, context, source_layout, path, &BTreeSet::new())
}

fn delete_previous_paths_under_except(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    source_layout: &scope::SourceLayoutDiscovery,
    path: &str,
    retained_paths: &BTreeSet<String>,
) -> Result<bool, CodeIndexError> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    let paths = context
        .previous_hashes
        .keys()
        .filter(|previous_path| previous_path.starts_with(&prefix))
        .filter(|previous_path| !retained_paths.contains(*previous_path))
        .filter(|previous_path| {
            path_is_selected_with_layout(
                previous_path,
                context.registration,
                context.selector,
                source_layout,
            )
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    if paths.len() > MAX_INCREMENTAL_GITLINK_EXPANDED_PATHS {
        return Err(CodeIndexError::InvalidInput(format!(
            "gitlink path {path} expands to {} files; run a full code index so the work is checkpointed and batched",
            paths.len()
        )));
    }
    if paths.is_empty() {
        return Ok(false);
    }
    build.deleted_paths.extend(paths);

    Ok(true)
}

struct ChangedPathParseContext<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    root: &'a Path,
    base_commit: &'a str,
    previous_hashes: &'a BTreeMap<String, String>,
    source_layout: &'a scope::SourceLayoutDiscovery,
    previous_source_layout: &'a scope::SourceLayoutDiscovery,
    effective_path_filters: &'a [String],
    prefetched_bytes: &'a BTreeMap<String, Vec<u8>>,
}

struct ChangedPathPrefetchRequest<'a> {
    registration: &'a CodeRepositoryRegistration,
    selector: &'a CodeRepositorySelector,
    root: &'a Path,
    commit: &'a str,
    changes: &'a [GitChange],
    head_entries: &'a [changes::GitTreeEntry],
    source_layout: &'a scope::SourceLayoutDiscovery,
    previous_source_layout: &'a scope::SourceLayoutDiscovery,
}

fn prefetch_changed_path_bytes(
    request: ChangedPathPrefetchRequest<'_>,
) -> Result<BTreeMap<String, Vec<u8>>, CodeIndexError> {
    let entries = request
        .head_entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.byte_count))
        .collect::<BTreeMap<_, _>>();
    let budget = CodeIndexResourceBudget::default();
    let mut paths = Vec::new();
    let mut total_bytes = 0usize;
    for path in request.changes.iter().filter_map(changed_head_path) {
        let Some(byte_count) = entries.get(path).copied() else {
            continue;
        };
        if !path_is_selected_with_layout(
            path,
            request.registration,
            request.selector,
            request.source_layout,
        ) && !path_is_selected_with_layout(
            path,
            request.registration,
            request.selector,
            request.previous_source_layout,
        ) {
            continue;
        }
        if paths.iter().any(|selected| selected == path) {
            continue;
        }
        if !paths.is_empty()
            && (paths.len() >= budget.max_files_per_batch
                || total_bytes.saturating_add(byte_count) > budget.max_bytes_per_batch)
        {
            break;
        }
        total_bytes = total_bytes.saturating_add(byte_count);
        paths.push(path.to_owned());
    }
    let blobs =
        source_batch_bytes_after_content_verification(request.root, request.commit, &paths, None)?;

    Ok(paths.into_iter().zip(blobs).collect())
}

fn changed_head_path(change: &GitChange) -> Option<&str> {
    match change {
        GitChange::AddedOrModified { path } | GitChange::TypeChanged { path } => Some(path),
        GitChange::Renamed { new_path, .. } | GitChange::Copied { new_path, .. } => Some(new_path),
        GitChange::Deleted { .. } => None,
    }
}

fn parse_changed_path(
    build: &mut SnapshotBuild,
    context: &ChangedPathParseContext<'_>,
    path: &str,
) -> Result<(), CodeIndexError> {
    if !path_is_selected_with_layout(
        path,
        context.registration,
        context.selector,
        context.source_layout,
    ) && !path_is_selected_with_layout(
        path,
        context.registration,
        context.selector,
        context.previous_source_layout,
    ) {
        return Ok(());
    }
    let bytes = match context.prefetched_bytes.get(path) {
        Some(bytes) => Cow::Borrowed(bytes.as_slice()),
        None => Cow::Owned(source_bytes_after_content_verification(
            context.root,
            &build.commit,
            path,
            None,
        )?),
    };
    let blob_hash = stable_content_hash(&bytes);
    if context.previous_hashes.get(path) == Some(&blob_hash) {
        build.skipped_unchanged_count += 1;
        return Ok(());
    }

    parse_indexed_file(build, path, &bytes)
}

fn gitlink_scope_overlaps(path: &str, context: &ChangedPathParseContext<'_>) -> bool {
    path_scope_overlaps(path, context.registration, context.selector)
        || scope::submodule_child_scope_filters_from_filters(path, context.effective_path_filters)
            .is_some()
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn changed_path_budget_accepts_boundary_and_rejects_excess() {
        validate_changed_path_budget(MAX_INCREMENTAL_CHANGED_PATHS)
            .expect("the configured boundary should be accepted");
        let error = validate_changed_path_budget(MAX_INCREMENTAL_CHANGED_PATHS + 1)
            .expect_err("an unbounded delta should be rejected");

        assert!(error.to_string().contains("run a full code index"));
    }
}
