use std::{path::Path, time::Duration};

use crate::code::{
    CodeIndexError,
    source::{
        change_status::{GitChange, parse_name_status_z},
        git::{GitNameStatusBudget, git_name_status_z_bounded, validate_git_ref_arg},
    },
};

pub(in crate::code) const MAX_GIT_DIFF_CHANGED_PATHS: usize = 100;
const MAX_GIT_DIFF_STDOUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_GIT_DIFF_STDERR_BYTES: usize = 64 * 1024;
const GIT_DIFF_TIMEOUT: Duration = Duration::from_secs(30);

pub(in crate::code) fn diff_changes(
    root: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<Vec<GitChange>, CodeIndexError> {
    validate_git_ref_arg("base_ref", base_ref)?;
    validate_git_ref_arg("head_ref", head_ref)?;
    let bytes = git_name_status_z_bounded(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--name-status",
            "--find-renames",
            "-z",
            "--end-of-options",
            base_ref,
            head_ref,
            "--",
        ],
        GitNameStatusBudget {
            max_paths: MAX_GIT_DIFF_CHANGED_PATHS,
            max_stdout_bytes: MAX_GIT_DIFF_STDOUT_BYTES,
            max_stderr_bytes: MAX_GIT_DIFF_STDERR_BYTES,
            timeout: GIT_DIFF_TIMEOUT,
        },
    )?;

    parse_name_status_z(&bytes)
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
