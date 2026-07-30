use std::{fs, path::Path};

use crate::code::source::{
    git::{git_bytes, resolve_ref},
    gitlink as source_gitlink,
};

use super::CodeIndexError;

pub(super) enum StagedPathKind {
    Gitlink(String),
    Regular,
}

pub(super) fn staged_path_kind(
    root: &Path,
    path: &str,
) -> Result<Option<StagedPathKind>, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-files", "-s", "-z", "--", path])?;
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
    if fields.first().copied() != Some("160000") {
        return Ok(Some(StagedPathKind::Regular));
    }

    Ok(fields
        .get(1)
        .map(|object| StagedPathKind::Gitlink((*object).to_owned())))
}

pub(super) fn submodule_worktree_parent_path(parent_path: &str, child_path: &str) -> String {
    format!("{}/{}", parent_path.trim_end_matches('/'), child_path)
}

pub(super) fn submodule_worktree_head(
    root: &Path,
    path: &str,
) -> Result<Option<String>, CodeIndexError> {
    let submodule_root = match source_gitlink::submodule_root(root, path) {
        Ok(submodule_root) => submodule_root,
        Err(_) => return Ok(None),
    };

    resolve_ref(&submodule_root, "HEAD").map(Some)
}

pub(super) fn base_path_exists(
    root: &Path,
    base_commit: &str,
    path: &str,
) -> Result<bool, CodeIndexError> {
    git_object_kind(root, base_commit, path).map(|kind| kind.is_some())
}

fn git_object_kind(
    root: &Path,
    commit: &str,
    path: &str,
) -> Result<Option<String>, CodeIndexError> {
    match git_bytes(root, ["cat-file", "-t", &format!("{commit}:{path}")]) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).trim().to_owned())),
        Err(_) => Ok(None),
    }
}

pub(super) fn contains_git_metadata(root: &Path, relative: &Path) -> Result<bool, CodeIndexError> {
    match fs::symlink_metadata(root.join(relative).join(".git")) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
