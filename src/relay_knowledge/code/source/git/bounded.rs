//! Runs small-output and NUL-framed Git commands with bounded output and lifetime.

use std::{
    io::{self, Read},
    path::Path,
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio},
    sync::mpsc::{self, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::code::CodeIndexError;

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Resource limits for one NUL-framed `git diff --name-status` command.
#[derive(Debug, Clone, Copy)]
pub(in crate::code) struct GitNameStatusBudget {
    pub(in crate::code) max_paths: usize,
    pub(in crate::code) max_stdout_bytes: usize,
    pub(in crate::code) max_stderr_bytes: usize,
    pub(in crate::code) timeout: Duration,
}

/// Resource limits for a Git command whose successful stdout must remain small.
#[derive(Debug, Clone, Copy)]
pub(in crate::code) struct GitSmallOutputBudget {
    pub(in crate::code) max_stdout_bytes: usize,
    pub(in crate::code) max_stderr_bytes: usize,
    pub(in crate::code) timeout: Duration,
}

/// Resource limits for a streaming NUL-framed Git record probe.
#[derive(Debug, Clone, Copy)]
pub(in crate::code) struct GitNulRecordBudget {
    pub(in crate::code) max_records: usize,
    pub(in crate::code) max_record_bytes: usize,
    pub(in crate::code) max_stderr_bytes: usize,
    pub(in crate::code) timeout: Duration,
}

#[derive(Debug, Clone, Copy)]
struct GitCommandBudget {
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    timeout: Duration,
}

#[derive(Debug, Clone, Copy)]
enum StdoutReadPolicy {
    Bytes,
    NameStatus { max_paths: usize },
}

#[derive(Debug, Clone, Copy)]
struct GitCommandPurpose {
    operation: &'static str,
    recovery_guidance: &'static str,
}

/// Collects a bounded Git name-status stream and terminates the child on overflow.
pub(in crate::code) fn git_name_status_z_bounded(
    root: &Path,
    args: &[&str],
    budget: GitNameStatusBudget,
) -> Result<Vec<u8>, CodeIndexError> {
    let error_args = args
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);

    run_name_status_command(command, error_args, budget)
}

/// Collects stdout for a Git identity command without allowing output growth.
pub(in crate::code) fn git_small_output_bounded(
    root: &Path,
    args: &[&str],
    budget: GitSmallOutputBudget,
    operation: &'static str,
) -> Result<Vec<u8>, CodeIndexError> {
    let error_args = args
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);

    run_small_output_command(command, error_args, budget, operation)
}

/// Streams NUL-framed stdout until a record matches or the bounded command ends.
pub(in crate::code) fn git_nul_records_match_bounded(
    root: &Path,
    args: &[&str],
    budget: GitNulRecordBudget,
    record_matches: fn(&[u8]) -> bool,
    operation: &'static str,
) -> Result<bool, CodeIndexError> {
    super::record_git_ls_tree_full_scan_call(root, args);
    let error_args = args
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect::<Vec<_>>();
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);

    run_nul_record_match_command(command, error_args, budget, record_matches, operation)
}

fn validate_command_budget(budget: GitCommandBudget) -> Result<(), CodeIndexError> {
    if budget.max_stdout_bytes == 0 || budget.max_stderr_bytes == 0 || budget.timeout.is_zero() {
        return Err(CodeIndexError::InvalidInput(
            "Git output limits and timeout must be greater than zero".to_owned(),
        ));
    }

    Ok(())
}

fn run_name_status_command(
    command: Command,
    error_args: Vec<String>,
    budget: GitNameStatusBudget,
) -> Result<Vec<u8>, CodeIndexError> {
    if budget.max_paths == 0 {
        return Err(CodeIndexError::InvalidInput(
            "Git name-status path limit must be greater than zero".to_owned(),
        ));
    }
    run_bounded_command(
        command,
        error_args,
        GitCommandBudget {
            max_stdout_bytes: budget.max_stdout_bytes,
            max_stderr_bytes: budget.max_stderr_bytes,
            timeout: budget.timeout,
        },
        StdoutReadPolicy::NameStatus {
            max_paths: budget.max_paths,
        },
        GitCommandPurpose {
            operation: "incremental Git diff",
            recovery_guidance: "; run a full code index",
        },
    )
}

fn run_small_output_command(
    command: Command,
    error_args: Vec<String>,
    budget: GitSmallOutputBudget,
    operation: &'static str,
) -> Result<Vec<u8>, CodeIndexError> {
    run_bounded_command(
        command,
        error_args,
        GitCommandBudget {
            max_stdout_bytes: budget.max_stdout_bytes,
            max_stderr_bytes: budget.max_stderr_bytes,
            timeout: budget.timeout,
        },
        StdoutReadPolicy::Bytes,
        GitCommandPurpose {
            operation,
            recovery_guidance: "",
        },
    )
}

fn run_nul_record_match_command(
    mut command: Command,
    error_args: Vec<String>,
    budget: GitNulRecordBudget,
    record_matches: fn(&[u8]) -> bool,
    operation: &'static str,
) -> Result<bool, CodeIndexError> {
    validate_nul_record_budget(budget)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = take_stdout(&mut child)?;
    let stderr = take_stderr(&mut child)?;
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(1);
    let stdout_reader = thread::spawn(move || {
        let result = read_nul_records_until_match(
            stdout,
            budget.max_records,
            budget.max_record_bytes,
            record_matches,
        );
        let _ = stdout_sender.send(result);
    });
    let stderr_reader = thread::spawn(move || read_bounded_stderr(stderr, budget.max_stderr_bytes));
    let deadline = Instant::now() + budget.timeout;
    let purpose = GitCommandPurpose {
        operation,
        recovery_guidance: "",
    };
    let mut completed_scan = None;

    let status = loop {
        if completed_scan.is_none() {
            match stdout_receiver.try_recv() {
                Ok(Ok(true)) => {
                    terminate_child(&mut child);
                    join_stdout_reader(stdout_reader)?;
                    let _ = join_stderr_reader(stderr_reader)?;
                    return Ok(true);
                }
                Ok(Ok(false)) => completed_scan = Some(false),
                Ok(Err(error)) => {
                    terminate_child(&mut child);
                    join_stdout_reader(stdout_reader)?;
                    let _ = join_stderr_reader(stderr_reader)?;
                    return Err(nul_record_read_error(error, operation));
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    terminate_child(&mut child);
                    join_stdout_reader(stdout_reader)?;
                    let _ = join_stderr_reader(stderr_reader)?;
                    return Err(CodeIndexError::InvalidInput(
                        "Git NUL-record reader stopped without a result".to_owned(),
                    ));
                }
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_child(&mut child);
                join_stdout_reader(stdout_reader)?;
                let _ = join_stderr_reader(stderr_reader)?;
                return Err(CodeIndexError::Io(error));
            }
        }
        let now = Instant::now();
        if now >= deadline {
            terminate_child(&mut child);
            join_stdout_reader(stdout_reader)?;
            let stderr = join_stderr_reader(stderr_reader)?;
            return Err(CodeIndexError::Git {
                args: error_args,
                message: timeout_message(purpose, budget.timeout, &stderr),
            });
        }
        thread::sleep(PROCESS_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    };

    let matched = match completed_scan {
        Some(matched) => Ok(matched),
        None => stdout_receiver.recv().map_err(|_| {
            CodeIndexError::InvalidInput(
                "Git NUL-record reader stopped without a result".to_owned(),
            )
        })?,
    };
    join_stdout_reader(stdout_reader)?;
    let stderr = join_stderr_reader(stderr_reader)?;
    let matched = matched.map_err(|error| nul_record_read_error(error, operation))?;
    if matched {
        return Ok(true);
    }
    if !status.success() {
        return Err(CodeIndexError::Git {
            args: error_args,
            message: failed_command_message(status, &stderr),
        });
    }

    Ok(false)
}

fn validate_nul_record_budget(budget: GitNulRecordBudget) -> Result<(), CodeIndexError> {
    if budget.max_records == 0
        || budget.max_record_bytes == 0
        || budget.max_stderr_bytes == 0
        || budget.timeout.is_zero()
    {
        return Err(CodeIndexError::InvalidInput(
            "Git NUL-record limits and timeout must be greater than zero".to_owned(),
        ));
    }

    Ok(())
}

fn run_bounded_command(
    mut command: Command,
    error_args: Vec<String>,
    budget: GitCommandBudget,
    stdout_policy: StdoutReadPolicy,
    purpose: GitCommandPurpose,
) -> Result<Vec<u8>, CodeIndexError> {
    validate_command_budget(budget)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = take_stdout(&mut child)?;
    let stderr = take_stderr(&mut child)?;
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(1);
    let stdout_reader = thread::spawn(move || {
        let result = read_bounded_stdout(stdout, stdout_policy, budget.max_stdout_bytes);
        let _ = stdout_sender.send(result);
    });
    let stderr_reader = thread::spawn(move || read_bounded_stderr(stderr, budget.max_stderr_bytes));
    let deadline = Instant::now() + budget.timeout;
    let mut completed_stdout = None;

    let status = loop {
        if completed_stdout.is_none() {
            match stdout_receiver.try_recv() {
                Ok(Ok(stdout)) => completed_stdout = Some(stdout),
                Ok(Err(error)) => {
                    terminate_child(&mut child);
                    join_stdout_reader(stdout_reader)?;
                    let _ = join_stderr_reader(stderr_reader)?;
                    return Err(stdout_read_error(error, purpose));
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    terminate_child(&mut child);
                    join_stdout_reader(stdout_reader)?;
                    let _ = join_stderr_reader(stderr_reader)?;
                    return Err(CodeIndexError::InvalidInput(
                        "Git stdout reader stopped without a result".to_owned(),
                    ));
                }
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate_child(&mut child);
                join_stdout_reader(stdout_reader)?;
                let _ = join_stderr_reader(stderr_reader)?;
                return Err(CodeIndexError::Io(error));
            }
        }
        let now = Instant::now();
        if now >= deadline {
            terminate_child(&mut child);
            join_stdout_reader(stdout_reader)?;
            let stderr = join_stderr_reader(stderr_reader)?;
            return Err(CodeIndexError::Git {
                args: error_args,
                message: timeout_message(purpose, budget.timeout, &stderr),
            });
        }
        thread::sleep(PROCESS_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    };

    let stdout = match completed_stdout {
        Some(stdout) => Ok(stdout),
        None => stdout_receiver.recv().map_err(|_| {
            CodeIndexError::InvalidInput("Git stdout reader stopped without a result".to_owned())
        })?,
    };
    join_stdout_reader(stdout_reader)?;
    let stderr = join_stderr_reader(stderr_reader)?;
    let stdout = stdout.map_err(|error| stdout_read_error(error, purpose))?;
    if !status.success() {
        return Err(CodeIndexError::Git {
            args: error_args,
            message: failed_command_message(status, &stderr),
        });
    }

    Ok(stdout)
}

fn take_stdout(child: &mut Child) -> Result<ChildStdout, CodeIndexError> {
    child.stdout.take().ok_or_else(|| {
        terminate_child(child);
        CodeIndexError::InvalidInput("Git child stdout is unavailable".to_owned())
    })
}

fn take_stderr(child: &mut Child) -> Result<ChildStderr, CodeIndexError> {
    child.stderr.take().ok_or_else(|| {
        terminate_child(child);
        CodeIndexError::InvalidInput("Git child stderr is unavailable".to_owned())
    })
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn join_stdout_reader(reader: JoinHandle<()>) -> Result<(), CodeIndexError> {
    reader
        .join()
        .map_err(|_| CodeIndexError::InvalidInput("Git stdout reader thread panicked".to_owned()))
}

fn join_stderr_reader(
    reader: JoinHandle<io::Result<BoundedStderr>>,
) -> Result<BoundedStderr, CodeIndexError> {
    reader
        .join()
        .map_err(|_| CodeIndexError::InvalidInput("Git stderr reader thread panicked".to_owned()))?
        .map_err(CodeIndexError::Io)
}

#[derive(Debug)]
enum StdoutReadError {
    Io(io::Error),
    ChangedPathLimit { observed: usize, limit: usize },
    ByteLimit { limit: usize },
}

fn read_bounded_stdout(
    stdout: ChildStdout,
    policy: StdoutReadPolicy,
    max_bytes: usize,
) -> Result<Vec<u8>, StdoutReadError> {
    match policy {
        StdoutReadPolicy::Bytes => read_byte_bounded_stdout(stdout, max_bytes),
        StdoutReadPolicy::NameStatus { max_paths } => {
            read_name_status_stdout(stdout, max_paths, max_bytes)
        }
    }
}

fn read_byte_bounded_stdout(
    mut stdout: impl Read,
    max_bytes: usize,
) -> Result<Vec<u8>, StdoutReadError> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stdout.read(&mut buffer).map_err(StdoutReadError::Io)?;
        if read == 0 {
            return Ok(output);
        }
        if read > max_bytes.saturating_sub(output.len()) {
            return Err(StdoutReadError::ByteLimit { limit: max_bytes });
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

fn read_name_status_stdout(
    mut stdout: impl Read,
    max_paths: usize,
    max_bytes: usize,
) -> Result<Vec<u8>, StdoutReadError> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut token_start = 0usize;
    let mut paths_remaining = 0_u8;
    let mut completed_paths = 0usize;
    loop {
        let read = stdout.read(&mut buffer).map_err(StdoutReadError::Io)?;
        if read == 0 {
            return Ok(output);
        }
        if read > max_bytes.saturating_sub(output.len()) {
            return Err(StdoutReadError::ByteLimit { limit: max_bytes });
        }
        output.extend_from_slice(&buffer[..read]);

        while let Some(relative_end) = output[token_start..].iter().position(|byte| *byte == 0) {
            let token_end = token_start + relative_end;
            let token = &output[token_start..token_end];
            token_start = token_end + 1;
            if token.is_empty() {
                continue;
            }
            if paths_remaining == 0 {
                paths_remaining = if matches!(token.first(), Some(b'R' | b'C')) {
                    2
                } else {
                    1
                };
                continue;
            }
            paths_remaining -= 1;
            completed_paths += 1;
            if completed_paths > max_paths {
                return Err(StdoutReadError::ChangedPathLimit {
                    observed: completed_paths,
                    limit: max_paths,
                });
            }
        }
    }
}

#[derive(Debug)]
enum NulRecordReadError {
    Io(io::Error),
    RecordLimit { observed: usize, limit: usize },
    RecordByteLimit { limit: usize },
    UnterminatedRecord,
}

fn read_nul_records_until_match(
    mut stdout: impl Read,
    max_records: usize,
    max_record_bytes: usize,
    record_matches: fn(&[u8]) -> bool,
) -> Result<bool, NulRecordReadError> {
    let mut record = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut completed_records = 0usize;
    loop {
        let read = stdout.read(&mut buffer).map_err(NulRecordReadError::Io)?;
        if read == 0 {
            return if record.is_empty() {
                Ok(false)
            } else {
                Err(NulRecordReadError::UnterminatedRecord)
            };
        }
        for byte in &buffer[..read] {
            if *byte == 0 {
                completed_records = completed_records.saturating_add(1);
                if completed_records > max_records {
                    return Err(NulRecordReadError::RecordLimit {
                        observed: completed_records,
                        limit: max_records,
                    });
                }
                if record_matches(&record) {
                    return Ok(true);
                }
                record.clear();
                continue;
            }
            if record.len() >= max_record_bytes {
                return Err(NulRecordReadError::RecordByteLimit {
                    limit: max_record_bytes,
                });
            }
            record.push(*byte);
        }
    }
}

#[derive(Debug)]
struct BoundedStderr {
    bytes: Vec<u8>,
    truncated: bool,
    limit: usize,
}

fn read_bounded_stderr(mut stderr: ChildStderr, max_bytes: usize) -> io::Result<BoundedStderr> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut truncated = false;
    loop {
        let read = stderr.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let retained = read.min(max_bytes.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }

    Ok(BoundedStderr {
        bytes,
        truncated,
        limit: max_bytes,
    })
}

fn stdout_read_error(error: StdoutReadError, purpose: GitCommandPurpose) -> CodeIndexError {
    match error {
        StdoutReadError::Io(error) => CodeIndexError::Io(error),
        StdoutReadError::ChangedPathLimit { observed, limit } => {
            CodeIndexError::InvalidInput(format!(
                "{} reached {observed} changed paths, exceeding the bounded limit of {limit}{}",
                purpose.operation, purpose.recovery_guidance
            ))
        }
        StdoutReadError::ByteLimit { limit } => CodeIndexError::InvalidInput(format!(
            "{} output exceeds the bounded limit of {limit} bytes{}",
            purpose.operation, purpose.recovery_guidance
        )),
    }
}

fn nul_record_read_error(error: NulRecordReadError, operation: &str) -> CodeIndexError {
    match error {
        NulRecordReadError::Io(error) => CodeIndexError::Io(error),
        NulRecordReadError::RecordLimit { observed, limit } => {
            CodeIndexError::InvalidInput(format!(
                "{operation} reached {observed} records, exceeding the bounded limit of {limit}"
            ))
        }
        NulRecordReadError::RecordByteLimit { limit } => CodeIndexError::InvalidInput(format!(
            "{operation} record exceeds the bounded limit of {limit} bytes"
        )),
        NulRecordReadError::UnterminatedRecord => CodeIndexError::InvalidInput(format!(
            "{operation} returned an unterminated NUL-framed record"
        )),
    }
}

fn timeout_message(
    purpose: GitCommandPurpose,
    timeout: Duration,
    stderr: &BoundedStderr,
) -> String {
    format!(
        "{} timed out after {} ms{}{}",
        purpose.operation,
        timeout.as_millis(),
        stderr_suffix(stderr),
        purpose.recovery_guidance
    )
}

fn failed_command_message(status: ExitStatus, stderr: &BoundedStderr) -> String {
    let detail = String::from_utf8_lossy(&stderr.bytes).trim().to_owned();
    let mut message = if detail.is_empty() {
        format!("Git exited with {status}")
    } else {
        detail
    };
    if stderr.truncated {
        message.push_str(&format!(" [stderr truncated to {} bytes]", stderr.limit));
    }
    message
}

fn stderr_suffix(stderr: &BoundedStderr) -> String {
    let detail = String::from_utf8_lossy(&stderr.bytes).trim().to_owned();
    if detail.is_empty() {
        String::new()
    } else if stderr.truncated {
        format!(": {detail} [stderr truncated to {} bytes]", stderr.limit)
    } else {
        format!(": {detail}")
    }
}

#[cfg(test)]
#[path = "bounded_tests.rs"]
mod tests;
