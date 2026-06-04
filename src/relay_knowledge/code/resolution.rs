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
    let filters = scoped_gitlink_filters(path_filters);
    if filters.is_empty() {
        return git_tree_has_gitlinks_under(root, commit, None);
    }

    for filter in filters {
        if git_tree_has_gitlink_overlapping_filter(root, commit, &filter)? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn git_tree_has_gitlink_overlapping_filter(
    root: &Path,
    commit: &str,
    filter: &str,
) -> Result<bool, CodeIndexError> {
    for ancestor in path_and_ancestors(filter) {
        if git_tree_exact_path_is_gitlink(root, commit, ancestor)? {
            return Ok(true);
        }
    }

    git_tree_has_gitlinks_under(root, commit, Some(filter))
}

fn git_tree_has_gitlinks_under(
    root: &Path,
    commit: &str,
    scope: Option<&str>,
) -> Result<bool, CodeIndexError> {
    let bytes = match scope {
        Some(scope) => git_bytes(root, ["ls-tree", "-r", "-z", commit, "--", scope])?,
        None => git_bytes(root, ["ls-tree", "-r", "-z", commit])?,
    };
    for record in split_nul(&bytes) {
        if git_tree_record_is_gitlink(&record) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn git_tree_exact_path_is_gitlink(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<bool, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-tree", "-z", commit, "--", path])?;

    Ok(split_nul(&bytes)
        .into_iter()
        .any(|record| git_tree_record_is_gitlink(&record)))
}

fn git_tree_record_is_gitlink(record: &str) -> bool {
    let Some((metadata, _)) = record.split_once('\t') else {
        return false;
    };
    let fields = metadata.split_whitespace().collect::<Vec<_>>();

    fields.get(1).copied() == Some("commit")
}

fn scoped_gitlink_filters(path_filters: &[String]) -> Vec<String> {
    let mut filters = Vec::new();
    for filter in path_filters {
        let normalized = normalize_path_filter(filter);
        if normalized.is_empty() {
            continue;
        }
        if normalized == "." {
            return Vec::new();
        }
        if !filters.iter().any(|existing| existing == normalized) {
            filters.push(normalized.to_owned());
        }
    }

    filters
}

fn path_and_ancestors(path: &str) -> Vec<&str> {
    let mut ancestors = Vec::new();
    let mut current = path;
    while !current.is_empty() {
        ancestors.push(current);
        let Some((parent, _)) = current.rsplit_once('/') else {
            break;
        };
        current = parent;
    }

    ancestors
}

fn normalize_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}
