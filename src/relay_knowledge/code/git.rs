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

pub(super) fn git_batch_blob_sizes(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    if paths
        .iter()
        .any(|path| path.contains('\n') || path.contains('\r'))
    {
        return paths
            .iter()
            .map(|path| {
                let object = format!("{commit}:{path}");
                match git_bytes(root, ["cat-file", "-s", &object]) {
                    Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().parse::<usize>().ok()),
                    Err(_) => Ok(None),
                }
            })
            .collect();
    }

    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch-check"])
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
            args: vec!["cat-file".to_owned(), "--batch-check".to_owned()],
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    parse_cat_file_batch_sizes(paths, &output.stdout)
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

fn parse_cat_file_batch_sizes(
    paths: &[String],
    bytes: &[u8],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    let lines = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.len() != paths.len() {
        return Err(CodeIndexError::InvalidInput(
            "git cat-file batch-check returned an unexpected row count".to_owned(),
        ));
    }

    let mut sizes = Vec::with_capacity(paths.len());
    for (path, line) in paths.iter().zip(lines) {
        let header = String::from_utf8_lossy(line);
        if header.ends_with(" missing") {
            sizes.push(None);
            continue;
        }
        let mut parts = header.split_whitespace();
        let _object = parts.next();
        let object_kind = parts.next();
        let size = parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| {
                CodeIndexError::InvalidInput(format!(
                    "git cat-file batch-check size is invalid for {path}"
                ))
            })?;
        sizes.push((object_kind == Some("blob")).then_some(size));
    }

    Ok(sizes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn batch_blob_sizes_report_missing_paths_without_failing_batch() {
        let repo = TestRepo::create("batch-blob-sizes");
        repo.write("src/alpha.rs", "pub fn alpha() {}\n");
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "base"]);
        let commit = repo.git_text(["rev-parse", "HEAD"]);

        let sizes = git_batch_blob_sizes(
            &repo.root,
            &commit,
            &["src/alpha.rs".to_owned(), "src/missing.rs".to_owned()],
        )
        .expect("batch blob sizes should load");

        assert_eq!(sizes, vec![Some("pub fn alpha() {}\n".len()), None]);
    }

    struct TestRepo {
        root: PathBuf,
    }

    impl TestRepo {
        fn create(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let root = std::env::temp_dir().join(format!(
                "relay-knowledge-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("repo directory should be created");
            let repo = Self { root };
            repo.git(["init"]);
            repo.git(["config", "user.email", "relay@example.invalid"]);
            repo.git(["config", "user.name", "Relay Test"]);
            repo
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory should exist");
            }
            fs::write(path, content).expect("fixture file should be written");
        }

        fn git<const N: usize>(&self, args: [&str; N]) {
            let output = Command::new("git")
                .current_dir(&self.root)
                .args(args)
                .output()
                .expect("git should run");
            assert!(
                output.status.success(),
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn git_text<const N: usize>(&self, args: [&str; N]) -> String {
            let output = Command::new("git")
                .current_dir(&self.root)
                .args(args)
                .output()
                .expect("git should run");
            assert!(
                output.status.success(),
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );

            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
