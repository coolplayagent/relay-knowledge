use std::{
    path::{Path, PathBuf},
    process::Command,
};

use super::CodeIndexError;

pub(super) fn resolve_git_root(path: &Path) -> Result<PathBuf, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Err(CodeIndexError::Git {
            args: vec!["rev-parse".to_owned(), "--show-toplevel".to_owned()],
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();

    Ok(PathBuf::from(root))
}

pub(super) fn resolve_ref(root: &Path, ref_selector: &str) -> Result<String, CodeIndexError> {
    validate_git_ref_arg("ref_selector", ref_selector)?;
    git_text(
        root,
        ["rev-parse", "--verify", "--end-of-options", ref_selector],
    )
}

pub(super) fn resolve_tree(root: &Path, commit: &str) -> Result<String, CodeIndexError> {
    git_text(root, ["rev-parse", &format!("{commit}^{{tree}}")])
}

pub(super) fn validate_git_ref_arg(field: &'static str, value: &str) -> Result<(), CodeIndexError> {
    if value.starts_with('-') {
        return Err(CodeIndexError::InvalidInput(format!(
            "{field} must not start with '-'"
        )));
    }

    Ok(())
}

fn git_text<const N: usize>(root: &Path, args: [&str; N]) -> Result<String, CodeIndexError> {
    let bytes = git_bytes(root, args)?;

    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

pub(super) fn git_optional<const N: usize>(
    root: &Path,
    args: [&str; N],
) -> Result<Option<String>, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }

    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

pub(super) fn git_bytes<const N: usize>(
    root: &Path,
    args: [&str; N],
) -> Result<Vec<u8>, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if output.status.success() {
        return Ok(output.stdout);
    }

    Err(CodeIndexError::Git {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

pub(super) fn git_object_exists(root: &Path, object: &str) -> Result<bool, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", object])
        .output()?;

    Ok(output.status.success())
}
