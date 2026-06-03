use std::path::Path;

use super::{
    CodeIndexError,
    changes::split_nul,
    git::{git_bytes, resolve_ref, resolve_tree},
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
        if git_tree_has_scoped_gitlinks(root, &commit, path_filters)? {
            let snapshot = scoped_source_snapshot_for_filters(
                root,
                ref_selector,
                path_filters,
                language_filters,
            )?;
            return Ok((snapshot.resolved_commit_sha, snapshot.tree_hash));
        }
        return Ok((commit.clone(), resolve_tree(root, &commit)?));
    }

    let snapshot =
        scoped_source_snapshot_for_filters(root, ref_selector, path_filters, language_filters)?;

    Ok((snapshot.resolved_commit_sha, snapshot.tree_hash))
}

fn git_tree_has_scoped_gitlinks(
    root: &Path,
    commit: &str,
    path_filters: &[String],
) -> Result<bool, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-tree", "-r", "-z", commit])?;
    for record in split_nul(&bytes) {
        let Some((metadata, path)) = record.split_once('\t') else {
            continue;
        };
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        if fields.get(1).copied() == Some("commit")
            && path_filters_allow_gitlink(path_filters, path)
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn path_filters_allow_gitlink(path_filters: &[String], path: &str) -> bool {
    path_filters.is_empty()
        || path_filters
            .iter()
            .any(|filter| path_overlaps_filter(path, filter))
}

fn path_overlaps_filter(path: &str, filter: &str) -> bool {
    let path = normalize_path_filter(path);
    let filter = normalize_path_filter(filter);
    if filter == "." {
        return true;
    }
    !path.is_empty()
        && !filter.is_empty()
        && (path == filter
            || path.starts_with(&format!("{filter}/"))
            || filter.starts_with(&format!("{path}/")))
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}
