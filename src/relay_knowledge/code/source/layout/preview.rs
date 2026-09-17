use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    code::{
        CodeIndexError, generated_detection, languages::language_id,
        parser::dependency_manifest_language_ids,
    },
    domain::{
        CodeRepositoryExcludedPath, CodeRepositoryLanguagePreview, CodeRepositoryLargestFile,
        CodeRepositoryRegistration, CodeRepositoryScopePreview, CodeRepositorySelector,
    },
};

use super::{
    discovery::discover_source_layout,
    scoped_snapshot::{
        filesystem_policy_for_selector, registration_allows_filesystem_ref,
        source_snapshot_for_scope,
    },
    selection::selection_exclusion_reason_for_source,
};

const PREVIEW_MAX_EXCLUDED_PATHS: usize = 50;
const PREVIEW_MAX_LARGEST_FILES: usize = 10;
const DEFAULT_TEXT_FILE_BUDGET_BYTES: usize = 512 * 1024;

/// Returns a non-mutating preview of the effective repository indexing scope.
pub(in crate::code) fn preview_repository_layout(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryScopePreview, CodeIndexError> {
    let root = PathBuf::from(&registration.root_path);
    let filesystem_policy = filesystem_policy_for_selector(registration, selector);
    let allow_filesystem_ref =
        registration_allows_filesystem_ref(registration, &root, &selector.ref_selector)?;
    let snapshot = source_snapshot_for_scope(
        &root,
        &selector.ref_selector,
        filesystem_policy,
        allow_filesystem_ref,
    )?;
    let mut selected_byte_count = 0usize;
    let mut selected_file_count = 0usize;
    let mut unsupported_file_count = 0usize;
    let mut generated_or_heavy_file_count = 0usize;
    let mut language_distribution = BTreeMap::<String, (usize, usize)>::new();
    let mut largest_files = Vec::<CodeRepositoryLargestFile>::new();
    let mut excluded_paths = Vec::<CodeRepositoryExcludedPath>::new();

    let entries = snapshot.entries;
    let source_layout = discover_source_layout(&entries);
    let mut selected_entries = Vec::new();
    for entry in entries {
        if let Some(reason) = selection_exclusion_reason_for_source(
            &entry.path,
            registration,
            selector,
            &source_layout,
            snapshot.kind,
        ) {
            if excluded_paths.len() < PREVIEW_MAX_EXCLUDED_PATHS {
                excluded_paths.push(CodeRepositoryExcludedPath {
                    path: entry.path,
                    reason,
                });
            }
            continue;
        }
        selected_entries.push(entry);
    }
    let (resolved_commit_sha, tree_hash, _) = if snapshot.kind.is_filesystem() {
        let mut hashes = snapshot.content_hashes;
        let selected = selected_entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        hashes.retain(|path, _| selected.contains(path.as_str()));
        let filters = super::effective_index_path_filters(registration, selector, &source_layout);
        let mut skipped = snapshot
            .skipped_paths
            .into_iter()
            .filter(|skipped| {
                if skipped.io.path_kind == crate::domain::CodePathKind::Directory {
                    super::path_overlaps_any_filter(&skipped.path, &filters)
                } else {
                    selection_exclusion_reason_for_source(
                        &skipped.path,
                        registration,
                        selector,
                        &source_layout,
                        snapshot.kind,
                    )
                    .is_none()
                }
            })
            .collect::<Vec<_>>();
        crate::code::source::complete_selected_filesystem_entries(
            &snapshot.root,
            &mut selected_entries,
            &mut hashes,
            &mut skipped,
        )?;
        let tree = crate::code::source::tree_hash_with_skipped(&hashes, &skipped);
        if crate::code::source::source_commit_is_filesystem(&selector.ref_selector)
            && selector.ref_selector != tree
        {
            return Err(CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {} no longer matches live indexed scope {tree}",
                selector.ref_selector
            )));
        }
        (tree.clone(), tree, hashes)
    } else {
        (
            snapshot.resolved_commit_sha,
            snapshot.tree_hash,
            BTreeMap::new(),
        )
    };
    for entry in &selected_entries {
        let language = preview_language_id(&entry.path);
        selected_file_count += 1;
        selected_byte_count = selected_byte_count.saturating_add(entry.byte_count);
        let bucket = language_distribution
            .entry(language.to_owned())
            .or_insert((0, 0));
        bucket.0 += 1;
        bucket.1 = bucket.1.saturating_add(entry.byte_count);
        let is_unsupported = language == "unknown";
        let is_generated = generated_detection::path_has_generated_signal(&entry.path);
        let is_heavy = entry.byte_count > DEFAULT_TEXT_FILE_BUDGET_BYTES;
        if is_unsupported {
            unsupported_file_count += 1;
        }
        if is_generated || is_heavy {
            generated_or_heavy_file_count += 1;
        }
        largest_files.push(CodeRepositoryLargestFile {
            path: entry.path.clone(),
            byte_count: entry.byte_count,
        });
    }
    largest_files.sort_by(|left, right| {
        right
            .byte_count
            .cmp(&left.byte_count)
            .then_with(|| left.path.cmp(&right.path))
    });
    largest_files.truncate(PREVIEW_MAX_LARGEST_FILES);

    Ok(CodeRepositoryScopePreview {
        repository_id: registration.repository_id.clone(),
        alias: registration.alias.clone(),
        requested_ref: selector.ref_selector.clone(),
        resolved_commit_sha,
        tree_hash,
        selected_file_count,
        selected_byte_count,
        unsupported_file_count,
        generated_or_heavy_file_count,
        // The index preview workflow fills this after validating parser batches.
        expected_degraded_file_count: 0,
        language_distribution: language_distribution
            .into_iter()
            .map(
                |(language_id, (file_count, byte_count))| CodeRepositoryLanguagePreview {
                    language_id,
                    file_count,
                    byte_count,
                },
            )
            .collect(),
        largest_files,
        excluded_paths,
    })
}

fn preview_language_id(path: &str) -> &'static str {
    language_id(path).unwrap_or_else(|| {
        dependency_manifest_language_ids(path)
            .and_then(|languages| languages.first().copied())
            .unwrap_or("unknown")
    })
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
