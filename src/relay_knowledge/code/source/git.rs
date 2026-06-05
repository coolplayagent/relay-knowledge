use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{ChildStderr, ChildStdin, ChildStdout, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
use std::sync::Mutex;

use super::CodeIndexError;

const GIT_CAT_FILE_BATCH_TIMEOUT: Duration = Duration::from_secs(120);
const GIT_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[cfg(test)]
static GIT_SHOW_OBSERVER: Mutex<Option<(PathBuf, usize)>> = Mutex::new(None);
#[cfg(test)]
static GIT_LS_TREE_FULL_SCAN_OBSERVER: Mutex<Option<(PathBuf, usize)>> = Mutex::new(None);

#[cfg(test)]
pub(crate) fn reset_git_show_call_count_for_root(root: PathBuf) {
    *GIT_SHOW_OBSERVER
        .lock()
        .expect("git show observer should lock") = Some((root, 0));
}

#[cfg(test)]
pub(crate) fn git_show_call_count_for_root(root: &Path) -> usize {
    GIT_SHOW_OBSERVER
        .lock()
        .expect("git show observer should lock")
        .as_ref()
        .filter(|(observed_root, _)| observed_root == root)
        .map(|(_, count)| *count)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn reset_git_ls_tree_full_scan_call_count_for_root(root: PathBuf) {
    *GIT_LS_TREE_FULL_SCAN_OBSERVER
        .lock()
        .expect("git ls-tree observer should lock") = Some((root, 0));
}

#[cfg(test)]
pub(crate) fn git_ls_tree_full_scan_call_count_for_root(root: &Path) -> usize {
    GIT_LS_TREE_FULL_SCAN_OBSERVER
        .lock()
        .expect("git ls-tree observer should lock")
        .as_ref()
        .filter(|(observed_root, _)| observed_root == root)
        .map(|(_, count)| *count)
        .unwrap_or(0)
}

pub(in crate::code) fn resolve_git_root(path: &Path) -> Result<PathBuf, CodeIndexError> {
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

pub(in crate::code) fn resolve_ref(
    root: &Path,
    ref_selector: &str,
) -> Result<String, CodeIndexError> {
    validate_git_ref_arg("ref_selector", ref_selector)?;
    git_text(
        root,
        ["rev-parse", "--verify", "--end-of-options", ref_selector],
    )
}

pub(in crate::code) fn resolve_tree(root: &Path, commit: &str) -> Result<String, CodeIndexError> {
    git_text(root, ["rev-parse", &format!("{commit}^{{tree}}")])
}

pub(in crate::code) fn validate_git_ref_arg(
    field: &'static str,
    value: &str,
) -> Result<(), CodeIndexError> {
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

pub(in crate::code) fn git_optional<const N: usize>(
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

pub(in crate::code) fn git_bytes<const N: usize>(
    root: &Path,
    args: [&str; N],
) -> Result<Vec<u8>, CodeIndexError> {
    git_bytes_slice(root, &args)
}

pub(in crate::code) fn git_bytes_slice(
    root: &Path,
    args: &[&str],
) -> Result<Vec<u8>, CodeIndexError> {
    record_git_show_call(root, args);
    record_git_ls_tree_full_scan_call(root, args);
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

fn record_git_show_call(_root: &Path, _args: &[&str]) {
    #[cfg(test)]
    {
        if _args.first().copied() != Some("show") {
            return;
        }
        if let Some((observed_root, count)) = GIT_SHOW_OBSERVER
            .lock()
            .expect("git show observer should lock")
            .as_mut()
            && observed_root == _root
        {
            *count += 1;
        }
    }
}

fn record_git_ls_tree_full_scan_call(_root: &Path, _args: &[&str]) {
    #[cfg(test)]
    {
        if _args.first().copied() != Some("ls-tree")
            || !_args.contains(&"-r")
            || _args.contains(&"--")
        {
            return;
        }
        if let Some((observed_root, count)) = GIT_LS_TREE_FULL_SCAN_OBSERVER
            .lock()
            .expect("git ls-tree observer should lock")
            .as_mut()
            && observed_root == _root
        {
            *count += 1;
        }
    }
}

pub(in crate::code) fn git_batch_blobs(
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

    let output = cat_file_output(root, ["cat-file", "--batch"], commit, paths)?;
    if !output.status.success() {
        return Err(CodeIndexError::Git {
            args: vec!["cat-file".to_owned(), "--batch".to_owned()],
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    parse_cat_file_batch(paths, &output.stdout)
}

pub(in crate::code) fn git_batch_blob_sizes(
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

    let output = cat_file_output(root, ["cat-file", "--batch-check"], commit, paths)?;
    if !output.status.success() {
        return Err(CodeIndexError::Git {
            args: vec!["cat-file".to_owned(), "--batch-check".to_owned()],
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    parse_cat_file_batch_sizes(paths, &output.stdout)
}

fn cat_file_output<const N: usize>(
    root: &Path,
    args: [&str; N],
    commit: &str,
    paths: &[String],
) -> Result<Output, CodeIndexError> {
    let input = cat_file_batch_input(commit, paths);
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);

    command_output_with_stdin(
        command,
        input,
        GIT_CAT_FILE_BATCH_TIMEOUT,
        args.iter().map(|arg| (*arg).to_owned()).collect(),
    )
}

fn cat_file_batch_input(commit: &str, paths: &[String]) -> Vec<u8> {
    let mut input = Vec::new();
    for path in paths {
        input.extend_from_slice(commit.as_bytes());
        input.push(b':');
        input.extend_from_slice(path.as_bytes());
        input.push(b'\n');
    }

    input
}

fn command_output_with_stdin(
    mut command: Command,
    input: Vec<u8>,
    timeout: Duration,
    timeout_args: Vec<String>,
) -> Result<Output, CodeIndexError> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| CodeIndexError::InvalidInput("child stdin is unavailable".to_owned()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CodeIndexError::InvalidInput("child stdout is unavailable".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CodeIndexError::InvalidInput("child stderr is unavailable".to_owned()))?;
    let stdin_writer = thread::spawn(move || write_stdin_and_close(stdin, input));
    let stdout_reader = thread::spawn(move || read_child_output(stdout));
    let stderr_reader = thread::spawn(move || read_child_error(stderr));
    let deadline = Instant::now() + timeout;

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdin_writer.join();
            let _ = stdout_reader.join();
            let stderr = stderr_reader
                .join()
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            return Err(CodeIndexError::Git {
                args: timeout_args,
                message: format!(
                    "timed out after {} ms{}",
                    timeout.as_millis(),
                    timeout_stderr_suffix(&stderr)
                ),
            });
        }
        thread::sleep(GIT_PROCESS_POLL_INTERVAL);
    };

    let stdin_result = stdin_writer
        .join()
        .map_err(|_| CodeIndexError::InvalidInput("child stdin writer panicked".to_owned()))?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| CodeIndexError::InvalidInput("child stdout reader panicked".to_owned()))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| CodeIndexError::InvalidInput("child stderr reader panicked".to_owned()))??;
    if status.success() {
        stdin_result?;
    }

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn write_stdin_and_close(mut stdin: ChildStdin, input: Vec<u8>) -> Result<(), std::io::Error> {
    stdin.write_all(&input)
}

fn read_child_output(mut stdout: ChildStdout) -> Result<Vec<u8>, std::io::Error> {
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes)?;

    Ok(bytes)
}

fn read_child_error(mut stderr: ChildStderr) -> Result<Vec<u8>, std::io::Error> {
    let mut bytes = Vec::new();
    stderr.read_to_end(&mut bytes)?;

    Ok(bytes)
}

fn timeout_stderr_suffix(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_owned();
    if message.is_empty() {
        String::new()
    } else {
        format!(": {message}")
    }
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

    #[test]
    fn piped_command_closes_stdin_before_waiting() {
        let output = command_output_with_stdin(
            git_hash_object_command(),
            b"alpha\n".to_vec(),
            Duration::from_secs(5),
            vec!["hash-object".to_owned(), "--stdin".to_owned()],
        )
        .expect("stdin-bound command should finish after EOF");

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "4a58007052a65fbc2fc3f910f2855f45a4058e74"
        );
    }

    fn git_hash_object_command() -> Command {
        let mut command = Command::new("git");
        command.args(["hash-object", "--stdin"]);
        command
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
