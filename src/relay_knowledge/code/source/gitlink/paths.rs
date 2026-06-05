use std::{collections::BTreeSet, path::PathBuf};

use super::{
    super::{CodeIndexError, changes::GitTreeEntry},
    selector::GitlinkPathSelector,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::code) struct SubmodulePathEntry {
    pub(in crate::code) parent_path: String,
    pub(in crate::code) child_path: String,
}

pub(in crate::code) struct SubmoduleChangedPathSets {
    pub(in crate::code) base_paths: BTreeSet<String>,
    pub(in crate::code) head_paths: BTreeSet<String>,
}

pub(in crate::code) struct GitlinkPathExpansion {
    pub(in crate::code) base_is_gitlink: bool,
    pub(in crate::code) head_is_gitlink: bool,
    pub(in crate::code) base_paths: BTreeSet<String>,
    pub(in crate::code) head_paths: BTreeSet<String>,
}

#[derive(Debug)]
pub(in crate::code) struct GitlinkTarget {
    pub(in crate::code) location: GitlinkTargetLocation,
    pub(in crate::code) commit: String,
    pub(in crate::code) path: String,
}

#[derive(Debug)]
pub(in crate::code) enum GitlinkTargetLocation {
    Worktree(PathBuf),
    GitDir(PathBuf),
}

pub(in crate::code) fn expanded_paths_under(
    entries: &[GitTreeEntry],
    path: &str,
) -> BTreeSet<String> {
    let prefix = format!("{}/", path.trim_end_matches('/'));
    entries
        .iter()
        .filter(|entry| entry.path.starts_with(&prefix))
        .map(|entry| entry.path.clone())
        .collect()
}

pub(in crate::code) fn bounded_expanded_paths_under_with_selector(
    entries: &[GitTreeEntry],
    path: &str,
    max_paths: usize,
    selector: &GitlinkPathSelector<'_>,
) -> Result<BTreeSet<String>, CodeIndexError> {
    let paths = expanded_paths_under(entries, path)
        .into_iter()
        .filter(|path| selector.includes(path))
        .collect::<BTreeSet<_>>();
    ensure_gitlink_expansion_budget(path, paths.len(), max_paths)?;

    Ok(paths)
}

pub(in crate::code) fn ensure_gitlink_expansion_budget(
    path: &str,
    expanded_count: usize,
    max_paths: usize,
) -> Result<(), CodeIndexError> {
    if expanded_count <= max_paths {
        return Ok(());
    }

    Err(CodeIndexError::InvalidInput(format!(
        "gitlink path {path} expands to {expanded_count} files; run a full code index so the work is checkpointed and batched"
    )))
}

pub(in crate::code) fn submodule_expansion_is_unavailable(error: &CodeIndexError) -> bool {
    match error {
        CodeIndexError::InvalidInput(message) => {
            message.contains("submodule git dir") && message.contains("unavailable")
        }
        CodeIndexError::Git { args, .. } => args.iter().any(|arg| arg == "ls-tree"),
        _ => false,
    }
}
