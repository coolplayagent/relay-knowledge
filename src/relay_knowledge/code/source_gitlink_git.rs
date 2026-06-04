use std::path::{Path, PathBuf};

use super::{
    CodeIndexError,
    changes::{git_dir_bytes, submodule_worktree_root},
    git::git_bytes,
};

pub(super) fn git_root_has_commit(root: &Path, commit: &str) -> bool {
    git_bytes(root, ["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_ok()
}

pub(super) fn submodule_worktree_root_for_commit(
    root: &Path,
    path: &str,
    commit: &str,
) -> Option<PathBuf> {
    submodule_worktree_root(root, path)
        .ok()
        .filter(|submodule_root| git_root_has_commit(submodule_root, commit))
}

#[derive(Debug)]
pub(super) struct GitTreeLookup {
    pub(super) object: String,
    pub(super) kind: String,
}

pub(super) fn git_tree_entry(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<GitTreeLookup>, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-tree", "-z", commit, "--", path])?;
    parse_git_tree_lookup(&bytes)
}

pub(super) fn git_tree_entry_from_git_dir(
    git_dir: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<GitTreeLookup>, CodeIndexError> {
    let bytes = git_dir_bytes(git_dir, &["ls-tree", "-z", commit, "--", path])?;
    parse_git_tree_lookup(&bytes)
}

fn parse_git_tree_lookup(bytes: &[u8]) -> Result<Option<GitTreeLookup>, CodeIndexError> {
    let Some(record) = bytes
        .split(|byte| *byte == 0)
        .find(|record| !record.is_empty())
    else {
        return Ok(None);
    };
    let record = String::from_utf8_lossy(record);
    let Some((metadata, _)) = record.split_once('\t') else {
        return Ok(None);
    };
    let fields = metadata.split_whitespace().collect::<Vec<_>>();
    let Some(object) = fields.get(2) else {
        return Ok(None);
    };

    Ok(Some(GitTreeLookup {
        object: (*object).to_owned(),
        kind: fields.get(1).copied().unwrap_or_default().to_owned(),
    }))
}

pub(super) fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains('\n')
        && !path.contains('\r')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
