//! Builds verified incremental snapshots for non-Git filesystem sources.

use std::{collections::BTreeMap, collections::BTreeSet, path::Path};

use crate::domain::{
    CodeIndexSnapshot, CodeRepositoryRegistration, CodeRepositorySelector,
    CodeWorkspaceDetectionConfig,
};

use super::{
    CodeIndexError,
    changes::GitTreeEntry,
    parser::parse_indexed_file,
    scope::{
        discover_source_layout, effective_index_path_filters_for_layouts,
        filesystem_policy_for_selector, scoped_source_snapshot_for_filters,
        selection_exclusion_reason_for_source,
    },
    snapshot::{SnapshotBuild, SnapshotScopeFilters},
    source::{
        RepositorySourceKind, ensure_filesystem_blobs_match_content_hashes,
        filesystem_source_snapshot, source_commit_is_filesystem, source_snapshot,
        source_snapshot_bytes, tree_hash_with_skipped,
    },
};

pub(crate) fn changed_paths_for_filesystem_diff(
    root: &Path,
    head_ref: &str,
    path_filters: &[String],
    language_filters: &[String],
    previous_hashes: &BTreeMap<String, String>,
) -> Result<Vec<String>, CodeIndexError> {
    let snapshot =
        scoped_source_snapshot_for_filters(root, head_ref, path_filters, language_filters)?;
    let mut changed_paths = BTreeSet::new();
    for (path, content_hash) in &snapshot.content_hashes {
        if previous_hashes.get(path) != Some(content_hash) {
            changed_paths.insert(path.clone());
        }
    }
    for path in previous_hashes.keys() {
        if !snapshot.content_hashes.contains_key(path) {
            changed_paths.insert(path.clone());
        }
    }

    Ok(changed_paths.into_iter().collect())
}

pub(super) fn build_filesystem_delta_snapshot(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    root: &Path,
    ref_selector: &str,
    previous_hashes: &BTreeMap<String, String>,
    base_resolved_commit_sha: Option<&str>,
    workspace_detection: &CodeWorkspaceDetectionConfig,
) -> Result<CodeIndexSnapshot, CodeIndexError> {
    let filesystem_policy = filesystem_policy_for_selector(registration, selector);
    let base_commit = base_resolved_commit_sha.ok_or_else(|| {
        CodeIndexError::InvalidInput(format!(
            "code repository '{}' filesystem incremental snapshot requires a previous resolved base commit",
            registration.repository_id
        ))
    })?;
    let snapshot =
        if source_commit_is_filesystem(ref_selector) || source_commit_is_filesystem(base_commit) {
            filesystem_source_snapshot(root, filesystem_policy.clone())?
        } else {
            source_snapshot(root, ref_selector, filesystem_policy.clone())?
        };
    let source_layout = discover_source_layout(&snapshot.entries);
    let previous_entries = previous_hashes
        .keys()
        .map(|path| GitTreeEntry {
            path: path.clone(),
            byte_count: 0,
        })
        .collect::<Vec<_>>();
    let previous_source_layout = discover_source_layout(&previous_entries);
    let path_filters = effective_index_path_filters_for_layouts(
        registration,
        selector,
        &[&source_layout, &previous_source_layout],
    );
    let language_filters = crate::domain::code_scope_language_filters(
        &registration.language_filters,
        &selector.language_filters,
    );
    let mut selected_entries = snapshot
        .entries
        .into_iter()
        .filter(|entry| {
            selection_exclusion_reason_for_source(
                &entry.path,
                registration,
                selector,
                &source_layout,
                RepositorySourceKind::FileSystem,
            )
            .is_none()
        })
        .collect::<Vec<_>>();
    let selected_paths = selected_entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let deleted_paths = previous_hashes
        .keys()
        .filter(|path| !selected_paths.contains(*path))
        .filter(|path| {
            selection_exclusion_reason_for_source(
                path,
                registration,
                selector,
                &source_layout,
                RepositorySourceKind::FileSystem,
            )
            .is_none()
                || selection_exclusion_reason_for_source(
                    path,
                    registration,
                    selector,
                    &previous_source_layout,
                    RepositorySourceKind::FileSystem,
                )
                .is_none()
        })
        .cloned()
        .collect::<Vec<_>>();
    let changed_path_count = selected_entries.len().saturating_add(deleted_paths.len());
    let mut planned_hashes = snapshot.content_hashes;
    planned_hashes.retain(|path, _| selected_paths.contains(path));
    let mut skipped = snapshot
        .skipped_paths
        .into_iter()
        .filter(|failure| {
            if failure.io.path_kind == crate::domain::CodePathKind::Directory {
                crate::code::source::layout::path_overlaps_any_filter(&failure.path, &path_filters)
            } else {
                [&source_layout, &previous_source_layout]
                    .iter()
                    .any(|layout| {
                        selection_exclusion_reason_for_source(
                            &failure.path,
                            registration,
                            selector,
                            layout,
                            RepositorySourceKind::FileSystem,
                        )
                        .is_none()
                    })
            }
        })
        .collect::<Vec<_>>();
    crate::code::source::complete_selected_filesystem_entries(
        &snapshot.root,
        &mut selected_entries,
        &mut planned_hashes,
        &mut skipped,
    )?;
    let initial_tree = tree_hash_with_skipped(&planned_hashes, &skipped);
    crate::code::source::ensure_filesystem_snapshot_matches_ref(ref_selector, &initial_tree)?;
    for _ in 0..=2 {
        std::fs::read_dir(&snapshot.root)?;
        let observed_skips = skipped.len();
        planned_hashes.retain(|path, _| !skipped.iter().any(|failure| failure.covers(path)));
        let tree_hash = tree_hash_with_skipped(&planned_hashes, &skipped);
        crate::code::source::ensure_filesystem_snapshot_matches_ref(ref_selector, &tree_hash)?;
        let mut build = SnapshotBuild::new_with_scope_filters(
            registration,
            tree_hash.clone(),
            tree_hash,
            SnapshotScopeFilters {
                path_filters: path_filters.clone(),
                language_filters: language_filters.clone(),
            },
            false,
            changed_path_count,
            0,
        );
        build.base_resolved_commit_sha = Some(base_commit.to_owned());
        build.deleted_paths = deleted_paths.clone();
        build.deleted_paths.extend(
            previous_hashes
                .keys()
                .filter(|path| skipped.iter().any(|failure| failure.covers(path)))
                .cloned(),
        );
        build.deleted_paths.sort();
        build.deleted_paths.dedup();
        let readable_entries = selected_entries
            .iter()
            .filter(|entry| !skipped.iter().any(|failure| failure.covers(&entry.path)))
            .cloned()
            .collect::<Vec<_>>();
        build.detect_and_fill_workspaces(
            root,
            RepositorySourceKind::FileSystem,
            &readable_entries,
            workspace_detection,
        );
        for entry in readable_entries {
            let bytes = match source_snapshot_bytes(
                &snapshot.root,
                RepositorySourceKind::FileSystem,
                &build.commit,
                &entry.path,
            ) {
                Ok(bytes) => bytes,
                Err(CodeIndexError::Io(error)) => {
                    skipped.push(crate::code::source::path_io::SkippedSourcePath::from_error(
                        &entry.path,
                        crate::domain::CodePathKind::File,
                        crate::domain::CodePathIoOperation::Read,
                        error,
                    )?);
                    continue;
                }
                Err(error) => return Err(error),
            };
            ensure_filesystem_blobs_match_content_hashes(
                &build.commit,
                std::slice::from_ref(&entry.path),
                std::slice::from_ref(&bytes),
                &planned_hashes,
            )?;
            let blob_hash = planned_hashes.get(&entry.path).ok_or_else(|| {
                CodeIndexError::Invariant(format!(
                    "filesystem plan is missing hash for {}",
                    entry.path
                ))
            })?;
            if previous_hashes.get(&entry.path) == Some(blob_hash) {
                build.skipped_unchanged_count += 1;
            } else {
                parse_indexed_file(&mut build, &entry.path, &bytes)?;
            }
        }
        if observed_skips == skipped.len() {
            build.diagnostics.extend(
                skipped
                    .iter()
                    .map(|path| path.diagnostic(&build.repository_id, &build.source_scope)),
            );
            return Ok(build.finish());
        }
    }
    Err(CodeIndexError::InvalidInput(
        "local source keeps changing beyond the bounded snapshot replan budget".into(),
    ))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
