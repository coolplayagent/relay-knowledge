use std::path::Path;

use super::{
    CodeIndexError,
    git::{resolve_ref, resolve_tree},
    scope::scoped_source_snapshot_for_filters,
    source::{source_commit_is_filesystem, source_kind},
};

pub fn resolve_repository_ref(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
) -> Result<String, CodeIndexError> {
    resolve_repository_ref_with_path_filters(root_path, ref_selector, &[])
}

pub fn resolve_repository_ref_with_path_filters(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
    path_filters: &[String],
) -> Result<String, CodeIndexError> {
    resolve_repository_ref_with_filters(root_path, ref_selector, path_filters, &[])
}

pub fn resolve_repository_ref_with_filters(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<String, CodeIndexError> {
    let root = root_path.as_ref();
    if source_commit_is_filesystem(ref_selector) {
        return Ok(ref_selector.to_owned());
    }
    if !source_kind(root)?.is_filesystem() {
        return resolve_ref(root, ref_selector);
    }

    Ok(
        scoped_source_snapshot_for_filters(root, ref_selector, path_filters, language_filters)?
            .resolved_commit_sha,
    )
}

pub fn resolve_repository_snapshot(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
) -> Result<(String, String), CodeIndexError> {
    resolve_repository_snapshot_with_path_filters(root_path, ref_selector, &[])
}

pub fn resolve_repository_snapshot_with_path_filters(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
    path_filters: &[String],
) -> Result<(String, String), CodeIndexError> {
    resolve_repository_snapshot_with_filters(root_path, ref_selector, path_filters, &[])
}

pub fn resolve_repository_snapshot_with_filters(
    root_path: impl AsRef<Path>,
    ref_selector: &str,
    path_filters: &[String],
    language_filters: &[String],
) -> Result<(String, String), CodeIndexError> {
    let root = root_path.as_ref();
    if source_commit_is_filesystem(ref_selector) {
        return Ok((ref_selector.to_owned(), ref_selector.to_owned()));
    }
    if !source_kind(root)?.is_filesystem() {
        let commit = resolve_ref(root, ref_selector)?;
        let tree_hash = resolve_tree(root, &commit)?;

        return Ok((commit, tree_hash));
    }

    let snapshot =
        scoped_source_snapshot_for_filters(root, ref_selector, path_filters, language_filters)?;

    Ok((snapshot.resolved_commit_sha, snapshot.tree_hash))
}
