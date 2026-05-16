use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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

pub(super) fn git_batch_blobs(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if paths
        .iter()
        .any(|path| path.contains('\n') || path.contains('\r'))
    {
        return paths
            .iter()
            .map(|path| git_bytes(root, ["show", &format!("{commit}:{path}")]))
            .collect();
    }

    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let stdin = child.stdin.as_mut().ok_or_else(|| {
            CodeIndexError::InvalidInput("git cat-file stdin is unavailable".to_owned())
        })?;
        for path in paths {
            writeln!(stdin, "{commit}:{path}")?;
        }
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(CodeIndexError::Git {
            args: vec!["cat-file".to_owned(), "--batch".to_owned()],
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    parse_cat_file_batch(paths, &output.stdout)
}

pub(super) fn git_object_exists(root: &Path, object: &str) -> Result<bool, CodeIndexError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", object])
        .output()?;

    Ok(output.status.success())
}

fn parse_cat_file_batch(paths: &[String], bytes: &[u8]) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    let mut offset = 0usize;
    let mut blobs = Vec::with_capacity(paths.len());
    for path in paths {
        let header_end = bytes[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|position| offset + position)
            .ok_or_else(|| {
                CodeIndexError::InvalidInput(format!(
                    "git cat-file batch header is missing for {path}"
                ))
            })?;
        let header = String::from_utf8_lossy(&bytes[offset..header_end]);
        let mut parts = header.split_whitespace();
        let _object = parts.next();
        let object_kind = parts.next();
        let size = parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| {
                CodeIndexError::InvalidInput(format!(
                    "git cat-file batch size is invalid for {path}"
                ))
            })?;
        if object_kind != Some("blob") {
            return Err(CodeIndexError::InvalidInput(format!(
                "git cat-file batch expected blob for {path}"
            )));
        }
        let content_start = header_end + 1;
        let content_end = content_start.checked_add(size).ok_or_else(|| {
            CodeIndexError::InvalidInput(format!("git cat-file blob size overflow for {path}"))
        })?;
        if bytes.len() < content_end + 1 {
            return Err(CodeIndexError::InvalidInput(format!(
                "git cat-file batch content is truncated for {path}"
            )));
        }
        blobs.push(bytes[content_start..content_end].to_vec());
        if bytes[content_end] != b'\n' {
            return Err(CodeIndexError::InvalidInput(format!(
                "git cat-file batch record terminator is missing for {path}"
            )));
        }
        offset = content_end + 1;
    }

    Ok(blobs)
}
