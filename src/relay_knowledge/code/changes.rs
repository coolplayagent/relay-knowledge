use std::path::Path;

use super::{
    CodeIndexError,
    git::{git_bytes, validate_git_ref_arg},
};

pub(super) fn tracked_entries(
    root: &Path,
    commit: &str,
) -> Result<Vec<GitTreeEntry>, CodeIndexError> {
    let bytes = git_bytes(root, ["ls-tree", "-r", "-l", "-z", commit])?;
    let mut entries = Vec::new();
    for record in split_nul(&bytes) {
        let Some((metadata, path)) = record.split_once('\t') else {
            continue;
        };
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        if fields.get(1) != Some(&"blob") {
            continue;
        }
        let byte_count = fields
            .get(3)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        entries.push(GitTreeEntry {
            path: path.to_owned(),
            byte_count,
        });
    }

    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GitTreeEntry {
    pub(super) path: String,
    pub(super) byte_count: usize,
}

pub(super) fn diff_changes(
    root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<GitChange>, CodeIndexError> {
    validate_git_ref_arg("base_ref", base_ref)?;
    validate_git_ref_arg("head_ref", head_ref)?;
    let bytes = git_bytes(
        root,
        [
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            "--end-of-options",
            base_ref,
            head_ref,
            "--",
        ],
    )?;

    parse_name_status_z(&bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GitChange {
    AddedOrModified { path: String },
    Deleted { path: String },
    Renamed { old_path: String, new_path: String },
    Copied { old_path: String, new_path: String },
    TypeChanged { path: String },
}

pub(super) fn parse_name_status_z(bytes: &[u8]) -> Result<Vec<GitChange>, CodeIndexError> {
    let tokens = split_nul(bytes);
    let mut changes = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let status = &tokens[index];
        index += 1;
        if status.starts_with('R') || status.starts_with('C') {
            let old_path = tokens.get(index).cloned().ok_or_else(|| {
                CodeIndexError::InvalidInput("rename old path is missing".to_owned())
            })?;
            let new_path = tokens.get(index + 1).cloned().ok_or_else(|| {
                CodeIndexError::InvalidInput("rename new path is missing".to_owned())
            })?;
            index += 2;
            if status.starts_with('R') {
                changes.push(GitChange::Renamed { old_path, new_path });
            } else {
                changes.push(GitChange::Copied { old_path, new_path });
            }
            continue;
        }

        let path = tokens
            .get(index)
            .cloned()
            .ok_or_else(|| CodeIndexError::InvalidInput("changed path is missing".to_owned()))?;
        index += 1;
        match status.chars().next() {
            Some('D') => changes.push(GitChange::Deleted { path }),
            Some('T') => changes.push(GitChange::TypeChanged { path }),
            Some('A' | 'M') => changes.push(GitChange::AddedOrModified { path }),
            _ => changes.push(GitChange::AddedOrModified { path }),
        }
    }

    Ok(changes)
}

pub(super) fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorktreePathChange {
    pub(super) status: String,
    pub(super) path: String,
    pub(super) deleted_source: Option<String>,
}

impl WorktreePathChange {
    pub(super) fn is_untracked(&self) -> bool {
        self.status == "??"
    }
}

pub(super) fn worktree_changed_paths(status: &[u8]) -> Vec<WorktreePathChange> {
    let tokens = split_nul(status);
    let mut changes = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.len() < 4 {
            index += 1;
            continue;
        }
        let status = &token[..2];
        let path = token[3..].to_owned();
        if (status.contains('R') || status.contains('C')) && tokens.get(index + 1).is_some() {
            let source = tokens[index + 1].clone();
            changes.push(WorktreePathChange {
                status: status.to_owned(),
                path,
                deleted_source: status.contains('R').then_some(source),
            });
            index += 2;
            continue;
        }
        changes.push(WorktreePathChange {
            status: status.to_owned(),
            path,
            deleted_source: None,
        });
        index += 1;
    }

    changes
}
