//! Reads bounded submodule status with Git-provided path types for early filtering.

use std::{path::Path, time::Duration};

use crate::code::{
    CodeIndexError,
    source::{
        changes::WorktreePathChange,
        git::{GitSmallOutputBudget, git_small_output_bounded},
    },
};

pub(super) struct TypedWorktreeChange {
    pub(super) change: WorktreePathChange,
    pub(super) known_file: bool,
}

pub(super) fn read_changes(root: &Path) -> Result<Vec<TypedWorktreeChange>, CodeIndexError> {
    let bytes = git_small_output_bounded(
        root,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        GitSmallOutputBudget {
            max_stdout_bytes: 8 * 1024 * 1024,
            max_stderr_bytes: 64 * 1024,
            timeout: Duration::from_secs(30),
        },
        "submodule worktree status",
    )?;
    parse_changes(&bytes)
}

fn parse_changes(bytes: &[u8]) -> Result<Vec<TypedWorktreeChange>, CodeIndexError> {
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    let mut changes = Vec::new();
    while let Some(record) = records.next() {
        let record = String::from_utf8_lossy(record);
        if let Some(path) = record.strip_prefix("? ") {
            changes.push(TypedWorktreeChange {
                change: WorktreePathChange {
                    status: "??".into(),
                    path: path.into(),
                    deleted_source: None,
                },
                known_file: false,
            });
            continue;
        }
        let (field_count, mode_end) = match record.as_bytes().first() {
            Some(b'1') => (9, 6),
            Some(b'2') => (10, 6),
            Some(b'u') => (11, 7),
            _ => return Err(invalid_status()),
        };
        let fields = record.splitn(field_count, ' ').collect::<Vec<_>>();
        if fields.len() != field_count || fields[1].len() != 2 || fields[field_count - 1].is_empty()
        {
            return Err(invalid_status());
        }
        let original = if fields[0] == "2" {
            Some(String::from_utf8_lossy(records.next().ok_or_else(invalid_status)?).into_owned())
        } else {
            None
        };
        let modes = &fields[3..mode_end];
        // Missing worktree entries can still be known files from the index/base.
        // A directory or any gitlink mode requires local boundary handling.
        let known_file = modes
            .iter()
            .all(|mode| matches!(*mode, "000000" | "100644" | "100755" | "120000"))
            && modes.iter().any(|mode| *mode != "000000");
        changes.push(TypedWorktreeChange {
            known_file,
            change: WorktreePathChange {
                status: fields[1].replace('.', " "),
                path: fields[field_count - 1].into(),
                deleted_source: original.filter(|_| fields[1].contains('R')),
            },
        });
    }
    Ok(changes)
}

fn invalid_status() -> CodeIndexError {
    CodeIndexError::Invariant("malformed Git porcelain v2 worktree status".into())
}

#[cfg(test)]
#[path = "worktree_status_tests.rs"]
mod tests;
