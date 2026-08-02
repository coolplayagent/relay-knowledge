use std::path::Path;

use crate::code::{
    CodeIndexError,
    source::{
        change_status::{GitChange, parse_name_status_z},
        git::{git_bytes, validate_git_ref_arg},
    },
};

pub(in crate::code) fn diff_changes(
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

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
