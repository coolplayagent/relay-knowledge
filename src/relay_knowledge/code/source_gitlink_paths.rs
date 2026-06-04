use std::{collections::BTreeSet, path::PathBuf};

use super::{CodeIndexError, changes::GitTreeEntry, source_gitlink_selector::GitlinkPathSelector};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SubmodulePathEntry {
    pub(super) parent_path: String,
    pub(super) child_path: String,
}

pub(super) struct SubmoduleChangedPathSets {
    pub(super) base_paths: BTreeSet<String>,
    pub(super) head_paths: BTreeSet<String>,
}

pub(super) struct GitlinkPathExpansion {
    pub(super) base_is_gitlink: bool,
    pub(super) head_is_gitlink: bool,
    pub(super) base_paths: BTreeSet<String>,
    pub(super) head_paths: BTreeSet<String>,
}

#[derive(Debug)]
pub(super) struct GitlinkTarget {
    pub(super) location: GitlinkTargetLocation,
    pub(super) commit: String,
    pub(super) path: String,
}

#[derive(Debug)]
pub(super) enum GitlinkTargetLocation {
    Worktree(PathBuf),
    GitDir(PathBuf),
}

pub(super) fn expanded_paths_under(entries: &[GitTreeEntry], path: &str) -> BTreeSet<String> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    entries
        .iter()
        .filter(|entry| entry.path.starts_with(&prefix))
        .map(|entry| entry.path.clone())
        .collect()
}

pub(super) fn bounded_expanded_paths_under_with_selector(
    entries: &[GitTreeEntry],
    path: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let paths = expanded_paths_under(entries, path)
        .into_iter()
        .filter(|path| selector.includes(path))
        .collect::<BTreeSet<_>>();
    if paths.len() > max_paths {
        return Err(CodeIndexError::InvalidInput(format!(
            "gitlink path {path} expands to {} files; run a full code index so the work is checkpointed and batched",
            paths.len()
        )));
    }

    Ok(paths)
}
