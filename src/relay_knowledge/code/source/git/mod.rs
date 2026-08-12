//! Bounded Git command execution, ref resolution, and batch blob access.

mod bounded;

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{ChildStderr, ChildStdin, ChildStdout, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
use std::sync::Mutex;

use super::CodeIndexError;

pub(in crate::code) use bounded::{
    GitNameStatusBudget, GitNulRecordBudget, GitSmallOutputBudget, git_name_status_z_bounded,
    git_nul_records_match_bounded, git_small_output_bounded,
};

const GIT_CAT_FILE_BATCH_TIMEOUT: Duration = Duration::from_secs(120);
const GIT_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);
const GIT_IDENTITY_TIMEOUT: Duration = Duration::from_secs(5);
const GIT_IDENTITY_STDOUT_LIMIT: usize = 256;
const GIT_ROOT_STDOUT_LIMIT: usize = 256 * 1024;
const GIT_IDENTITY_STDERR_LIMIT: usize = 16 * 1024;
const GIT_WORKTREE_STATUS_TIMEOUT: Duration = Duration::from_secs(30);
const GIT_WORKTREE_STATUS_RECORD_LIMIT: usize = 513;
const GIT_WORKTREE_STATUS_STDERR_LIMIT: usize = 64 * 1024;
const GIT_WORKTREE_STATUS_STDOUT_LIMIT: usize = 8 * 1024 * 1024;
const GIT_WORKTREE_OBSERVATION_BYTE_LIMIT: usize =
    crate::domain::CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH;

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
    let root = git_small_output_bounded(
        path,
        &["rev-parse", "--show-toplevel"],
        GitSmallOutputBudget {
            max_stdout_bytes: GIT_ROOT_STDOUT_LIMIT,
            max_stderr_bytes: GIT_IDENTITY_STDERR_LIMIT,
            timeout: GIT_IDENTITY_TIMEOUT,
        },
        "Git root resolution",
    )?;
    let root = String::from_utf8_lossy(&root).trim().to_owned();
    if root.is_empty() {
        return Err(CodeIndexError::InvalidInput(
            "Git root resolution returned an empty path".to_owned(),
        ));
    }

    Ok(PathBuf::from(root))
}

pub(in crate::code) fn resolve_ref(
    root: &Path,
    ref_selector: &str,
) -> Result<String, CodeIndexError> {
    resolve_git_ref_bounded(root, ref_selector)
}

/// Resolves a ref to one immutable full commit object ID with bounded I/O.
pub(crate) fn resolve_git_ref_bounded(
    root: &Path,
    ref_selector: &str,
) -> Result<String, CodeIndexError> {
    validate_git_ref_arg("ref_selector", ref_selector)?;
    let commit_selector = format!("{ref_selector}^{{commit}}");
    bounded_git_object_id(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &commit_selector,
        ],
        "Git ref resolution",
    )
}

pub(in crate::code) fn resolve_tree(root: &Path, commit: &str) -> Result<String, CodeIndexError> {
    resolve_git_tree_bounded(root, commit)
}

/// Resolves the tree for a pinned full commit object ID with bounded I/O.
pub(crate) fn resolve_git_tree_bounded(
    root: &Path,
    commit: &str,
) -> Result<String, CodeIndexError> {
    validate_full_git_object_id("commit", commit)?;
    let tree_selector = format!("{commit}^{{commit}}^{{tree}}");
    bounded_git_object_id(
        root,
        &["rev-parse", "--verify", "--end-of-options", &tree_selector],
        "Git tree resolution",
    )
}

/// Derives a stable, bounded identity for tracked and untracked worktree changes.
pub(crate) fn repository_worktree_observation_bounded(
    root: &Path,
) -> Result<Option<u64>, CodeIndexError> {
    let status = git_small_output_bounded(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        GitSmallOutputBudget {
            max_stdout_bytes: GIT_WORKTREE_STATUS_STDOUT_LIMIT,
            max_stderr_bytes: GIT_WORKTREE_STATUS_STDERR_LIMIT,
            timeout: GIT_WORKTREE_STATUS_TIMEOUT,
        },
        "Git worktree status observation",
    )?;
    if status.is_empty() {
        return Ok(None);
    }
    let changes = super::change_status::worktree_changed_paths(&status);
    if changes.len() > GIT_WORKTREE_STATUS_RECORD_LIMIT - 1 {
        return Err(CodeIndexError::InvalidInput(format!(
            "worktree observation exceeds {} changed paths; commit changes or run a full code index",
            GIT_WORKTREE_STATUS_RECORD_LIMIT - 1
        )));
    }
    let mut identity = WorktreeObservationHash::new();
    identity.update(&status);
    let mut observed_bytes = 0usize;
    for change in changes {
        identity.update(change.path.as_bytes());
        let path = root.join(&change.path);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                let byte_len = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
                observed_bytes = observed_bytes.saturating_add(byte_len);
                if observed_bytes > GIT_WORKTREE_OBSERVATION_BYTE_LIMIT {
                    return Err(CodeIndexError::InvalidInput(format!(
                        "worktree observation exceeds the {} byte budget; commit changes or run a full code index",
                        GIT_WORKTREE_OBSERVATION_BYTE_LIMIT
                    )));
                }
                let mut file = fs::File::open(path)?;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let read = file.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    identity.update(&buffer[..read]);
                }
            }
            Ok(metadata) if metadata.file_type().is_symlink() => {
                identity.update(fs::read_link(path)?.as_os_str().as_encoded_bytes());
            }
            Ok(metadata) => {
                identity.update(&metadata.len().to_le_bytes());
                identity.update(
                    &metadata
                        .modified()
                        .ok()
                        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|value| value.as_nanos())
                        .unwrap_or_default()
                        .to_le_bytes(),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                identity.update(b"deleted")
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(Some(identity.finish()))
}

struct WorktreeObservationHash(u64);

impl WorktreeObservationHash {
    const fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(0x100000001b3);
    }

    const fn finish(self) -> u64 {
        self.0
    }
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

fn bounded_git_object_id(
    root: &Path,
    args: &[&str],
    operation: &'static str,
) -> Result<String, CodeIndexError> {
    let bytes = git_small_output_bounded(
        root,
        args,
        GitSmallOutputBudget {
            max_stdout_bytes: GIT_IDENTITY_STDOUT_LIMIT,
            max_stderr_bytes: GIT_IDENTITY_STDERR_LIMIT,
            timeout: GIT_IDENTITY_TIMEOUT,
        },
        operation,
    )?;
    let object_id = String::from_utf8_lossy(&bytes).trim().to_owned();
    validate_full_git_object_id("resolved Git object", &object_id)?;

    Ok(object_id)
}

fn validate_full_git_object_id(field: &str, value: &str) -> Result<(), CodeIndexError> {
    if matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }

    Err(CodeIndexError::InvalidInput(format!(
        "{field} must be a full SHA-1 or SHA-256 Git object ID"
    )))
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

pub(in crate::code) fn git_dir_bytes(
    git_dir: &Path,
    args: &[&str],
) -> Result<Vec<u8>, CodeIndexError> {
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(git_dir)
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
    git_batch_blobs_without_fallback(root, commit, paths)
        .or_else(|_| git_blobs_one_path_at_a_time(root, commit, paths))
}

pub(in crate::code) fn git_batch_blobs_without_fallback(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    if paths
        .iter()
        .any(|path| path.contains('\n') || path.contains('\r'))
    {
        return Err(CodeIndexError::InvalidInput(
            "git cat-file batch paths must not contain line separators".to_owned(),
        ));
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
        return git_blob_sizes_one_path_at_a_time(root, commit, paths);
    }

    let output = cat_file_output(root, ["cat-file", "--batch-check"], commit, paths)?;
    if !output.status.success() {
        return Err(CodeIndexError::Git {
            args: vec!["cat-file".to_owned(), "--batch-check".to_owned()],
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    parse_cat_file_batch_sizes(paths, &output.stdout)
        .or_else(|_| git_blob_sizes_one_path_at_a_time(root, commit, paths))
}

fn git_blobs_one_path_at_a_time(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Vec<u8>>, CodeIndexError> {
    paths
        .iter()
        .map(|path| git_bytes(root, ["show", &format!("{commit}:{path}")]))
        .collect()
}

fn git_blob_sizes_one_path_at_a_time(
    root: &Path,
    commit: &str,
    paths: &[String],
) -> Result<Vec<Option<usize>>, CodeIndexError> {
    paths
        .iter()
        .map(|path| {
            let object = format!("{commit}:{path}");
            match git_bytes(root, ["cat-file", "-s", &object]) {
                Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().parse::<usize>().ok()),
                Err(_) => Ok(None),
            }
        })
        .collect()
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
        if header.ends_with(" missing") {
            return Err(CodeIndexError::InvalidInput(format!(
                "git cat-file batch object is missing for {path}"
            )));
        }
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
#[path = "mod_tests.rs"]
mod tests;
