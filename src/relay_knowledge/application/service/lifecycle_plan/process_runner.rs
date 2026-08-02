//! Bounded external-command execution and pipe draining.

use std::{
    io::Read,
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SERVICE_LIFECYCLE_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const SERVICE_LIFECYCLE_COMMAND_OUTPUT_LIMIT: usize = 64 * 1024;
const SERVICE_LIFECYCLE_COMMAND_OUTPUT_JOIN_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn run_command(command: &[String]) -> Result<String, String> {
    run_command_with_timeout(command, SERVICE_LIFECYCLE_COMMAND_TIMEOUT)
}

pub(super) fn run_command_with_timeout(
    command: &[String],
    timeout: Duration,
) -> Result<String, String> {
    let Some(program) = command.first() else {
        return Ok("no external command".to_owned());
    };
    let mut child = Command::new(program)
        .args(&command[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .map(|pipe| drain_pipe_limited(pipe, SERVICE_LIFECYCLE_COMMAND_OUTPUT_LIMIT));
    let mut stderr = child
        .stderr
        .take()
        .map(|pipe| drain_pipe_limited(pipe, SERVICE_LIFECYCLE_COMMAND_OUTPUT_LIMIT));
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            let output = collect_child_output(
                stdout.take(),
                stderr.take(),
                SERVICE_LIFECYCLE_COMMAND_OUTPUT_JOIN_TIMEOUT,
            );
            if status.success() {
                return Ok(format!("exit_status={status}"));
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            return Err(if detail.is_empty() {
                format!("exit_status={status}")
            } else {
                detail.to_owned()
            });
        }
        if Instant::now() >= deadline {
            terminate_child(&mut child);
            let _ = collect_child_output(
                stdout.take(),
                stderr.take(),
                SERVICE_LIFECYCLE_COMMAND_OUTPUT_JOIN_TIMEOUT,
            );
            return Err(format!("command timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

struct CommandOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn drain_pipe_limited<R>(mut pipe: R, limit: usize) -> JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut retained = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => return retained,
                Ok(read) => {
                    let remaining = limit.saturating_sub(retained.len());
                    if remaining > 0 {
                        retained.extend_from_slice(&buffer[..read.min(remaining)]);
                    }
                }
                Err(_) => return retained,
            }
        }
    })
}

fn collect_child_output(
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    timeout: Duration,
) -> CommandOutput {
    let deadline = Instant::now() + timeout;
    let stderr = join_output_until(stderr, deadline);
    let stdout = join_output_until(stdout, deadline);
    CommandOutput { stdout, stderr }
}

fn join_output_until(handle: Option<JoinHandle<Vec<u8>>>, deadline: Instant) -> Vec<u8> {
    let Some(handle) = handle else {
        return Vec::new();
    };
    while !handle.is_finished() {
        let now = Instant::now();
        if now >= deadline {
            return Vec::new();
        }
        thread::sleep((deadline - now).min(Duration::from_millis(10)));
    }
    handle.join().unwrap_or_default()
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
#[path = "process_runner_tests.rs"]
mod tests;
