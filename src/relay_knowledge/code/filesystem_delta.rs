use std::{collections::BTreeMap, collections::BTreeSet, path::Path};

use crate::domain::{CodeIndexSnapshot, CodeRepositoryRegistration, CodeRepositorySelector};

use super::{
    CodeIndexError,
    changes::GitTreeEntry,
    parser::parse_indexed_file,
    scope::{
        discover_source_layout, effective_index_path_filters_for_layouts,
        filesystem_policy_for_selector, scoped_source_snapshot_for_filters,
        selection_exclusion_reason_for_source,
    },
    snapshot::{self, SnapshotBuild, SnapshotScopeFilters},
    source::{
        RepositorySourceKind, ensure_filesystem_blobs_match_content_hashes,
        filesystem_content_hashes_for_paths, filesystem_source_snapshot,
        filesystem_tree_hash_from_path_hashes, source_commit_is_filesystem, source_snapshot,
        source_snapshot_bytes,
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
    let language_filters =
        snapshot::merged_filters(&registration.language_filters, &selector.language_filters);
    let selected_entries = snapshot
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
    let selected_path_list = selected_paths.iter().cloned().collect::<Vec<_>>();
    let planned_hashes = filesystem_content_hashes_for_paths(&snapshot.root, &selected_path_list)?;
    let tree_hash = filesystem_tree_hash_from_path_hashes(&planned_hashes);
    if source_commit_is_filesystem(ref_selector) && ref_selector != tree_hash {
        return Err(CodeIndexError::InvalidInput(format!(
            "filesystem source snapshot {ref_selector} no longer matches live indexed scope {tree_hash}"
        )));
    }
    let mut build = SnapshotBuild::new_with_scope_filters(
        registration,
        tree_hash.clone(),
        tree_hash,
        SnapshotScopeFilters {
            path_filters,
            language_filters,
        },
        false,
        changed_path_count,
        0,
    );
    build.base_resolved_commit_sha = Some(base_commit.to_owned());
    build.deleted_paths = deleted_paths;

    for entry in selected_entries {
        let bytes = source_snapshot_bytes(
            &snapshot.root,
            RepositorySourceKind::FileSystem,
            &build.commit,
            &entry.path,
        )?;
        ensure_filesystem_blobs_match_content_hashes(
            &build.commit,
            std::slice::from_ref(&entry.path),
            std::slice::from_ref(&bytes),
            &planned_hashes,
        )?;
        let blob_hash = planned_hashes.get(&entry.path).ok_or_else(|| {
            CodeIndexError::InvalidInput(format!(
                "filesystem source snapshot {} is missing planned content hash for {}",
                build.commit, entry.path
            ))
        })?;
        if previous_hashes.get(&entry.path) == Some(blob_hash) {
            build.skipped_unchanged_count += 1;
            continue;
        }
        parse_indexed_file(&mut build, &entry.path, &bytes)?;
    }

    Ok(build.finish())
}

#[cfg(test)]
mod tests {
    use super::{
        super::test_fixtures::TempSourceDir, changed_paths_for_filesystem_diff,
        filesystem_content_hashes_for_paths,
    };

    #[test]
    fn filesystem_diff_reports_deleted_base_paths() {
        let source = TempSourceDir::create("filesystem-diff-deletion");
        source.write("src/lib.rs", "pub fn unchanged() {}\n");
        source.write("src/api.rs", "pub fn removed() {}\n");
        let paths = vec!["src/api.rs".to_owned(), "src/lib.rs".to_owned()];
        let previous_hashes = filesystem_content_hashes_for_paths(&source.path, &paths)
            .expect("base filesystem hashes should compute");
        std::fs::remove_file(source.path.join("src/api.rs")).expect("indexed file should delete");

        let changed_paths =
            changed_paths_for_filesystem_diff(&source.path, "HEAD", &[], &[], &previous_hashes)
                .expect("filesystem diff should compare against stored base hashes");

        assert_eq!(changed_paths, ["src/api.rs".to_owned()]);
    }
}
