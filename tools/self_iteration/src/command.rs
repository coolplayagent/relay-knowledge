use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

const COMMAND_PROGRESS_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub name: String,
    pub command: Vec<String>,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

impl CommandResult {
    pub fn passed(&self) -> bool {
        self.exit_code == 0
    }

    pub fn gate_message(&self) -> String {
        last_output_line(&self.stdout, &self.stderr)
    }

    pub fn serializable(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "command": self.command,
            "exit_code": self.exit_code,
            "duration_ms": self.duration_ms,
            "stdout_tail": tail(&self.stdout, 4000),
            "stderr_tail": tail(&self.stderr, 4000),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub name: String,
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub env: Option<BTreeMap<String, String>>,
    pub timeout_seconds: u64,
    pub stdin: Option<String>,
}

impl CommandSpec {
    pub fn new<N: Into<String>>(
        name: N,
        command: Vec<String>,
        cwd: &Path,
        env: Option<BTreeMap<String, String>>,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            name: name.into(),
            command,
            cwd: cwd.to_path_buf(),
            env,
            timeout_seconds,
            stdin: None,
        }
    }

    pub fn with_stdin(mut self, stdin: String) -> Self {
        self.stdin = Some(stdin);
        self
    }
}

pub fn inherited_env() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

pub fn run_command(spec: &CommandSpec) -> CommandResult {
    let started = Instant::now();
    let Some(program) = spec.command.first() else {
        log_command_invalid(spec, started, "empty command");
        return failed_result(spec, 1, started, "empty command");
    };
    log_command_started(spec);
    let mut command = Command::new(program);
    command.args(spec.command.iter().skip(1));
    command.current_dir(&spec.cwd);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    if spec.stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    if let Some(env) = &spec.env {
        command.env_clear();
        command.envs(env);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let message = error.to_string();
            log_command_invalid(spec, started, &message);
            return failed_result(spec, 1, started, &message);
        }
    };
    let stdout_reader = child.stdout.take().map(read_pipe);
    let stderr_reader = child.stderr.take().map(read_pipe);
    let stdin_writer = spec.stdin.as_ref().and_then(|stdin| {
        child
            .stdin
            .take()
            .map(|handle| write_pipe(handle, stdin.clone()))
    });
    let timeout = Duration::from_secs(spec.timeout_seconds);
    let mut next_progress = COMMAND_PROGRESS_INTERVAL;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_reader(stdout_reader);
                let mut stderr = join_reader(stderr_reader);
                append_stdin_error(&mut stderr, stdin_writer);
                let result = CommandResult {
                    name: spec.name.clone(),
                    command: spec.command.clone(),
                    exit_code: status.code().unwrap_or(1),
                    duration_ms: started.elapsed().as_millis() as u64,
                    stdout,
                    stderr,
                };
                log_command_finished(&result);
                return result;
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = join_reader(stdout_reader);
                let mut stderr = join_reader(stderr_reader);
                append_stdin_error(&mut stderr, stdin_writer);
                stderr.push_str(&format!("\ntimeout after {}s", spec.timeout_seconds));
                let result = CommandResult {
                    name: spec.name.clone(),
                    command: spec.command.clone(),
                    exit_code: 124,
                    duration_ms: started.elapsed().as_millis() as u64,
                    stdout,
                    stderr,
                };
                log_command_timeout(&result, spec.timeout_seconds);
                return result;
            }
            Ok(None) => {
                let elapsed = started.elapsed();
                if elapsed >= next_progress {
                    log_command_running(spec, elapsed);
                    while next_progress <= elapsed {
                        next_progress += COMMAND_PROGRESS_INTERVAL;
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                let message = error.to_string();
                log_command_invalid(spec, started, &message);
                return failed_result(spec, 1, started, &message);
            }
        }
    }
}

fn read_pipe<R>(mut reader: R) -> JoinHandle<String>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut output = String::new();
        let _ = reader.read_to_string(&mut output);
        output
    })
}

fn write_pipe<W>(mut writer: W, input: String) -> JoinHandle<Result<(), String>>
where
    W: Write + Send + 'static,
{
    std::thread::spawn(move || {
        writer
            .write_all(input.as_bytes())
            .map_err(|error| error.to_string())
    })
}

fn join_reader(reader: Option<JoinHandle<String>>) -> String {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

fn append_stdin_error(stderr: &mut String, writer: Option<JoinHandle<Result<(), String>>>) {
    let Some(writer) = writer else {
        return;
    };
    let error = match writer.join() {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        Err(_) => "stdin writer thread panicked".to_owned(),
    };
    if !stderr.is_empty() {
        stderr.push('\n');
    }
    stderr.push_str("stdin write failed: ");
    stderr.push_str(&error);
}

fn log_command_started(spec: &CommandSpec) {
    eprintln!(
        "[self-iterate] command start name={} program={} argc={} timeout_s={}",
        spec.name,
        compact_log_text(command_program(spec), 120),
        spec.command.len(),
        spec.timeout_seconds
    );
}

fn log_command_running(spec: &CommandSpec, elapsed: Duration) {
    eprintln!(
        "[self-iterate] command running name={} elapsed_s={} timeout_s={}",
        spec.name,
        elapsed.as_secs(),
        spec.timeout_seconds
    );
}

fn log_command_finished(result: &CommandResult) {
    let status = if result.passed() { "ok" } else { "failed" };
    let message = result.gate_message();
    if result.passed() || message.is_empty() {
        eprintln!(
            "[self-iterate] command done name={} status={} exit={} duration_ms={}",
            result.name, status, result.exit_code, result.duration_ms
        );
    } else {
        eprintln!(
            "[self-iterate] command done name={} status={} exit={} duration_ms={} message={:?}",
            result.name,
            status,
            result.exit_code,
            result.duration_ms,
            compact_log_text(&message, 240)
        );
    }
}

fn log_command_timeout(result: &CommandResult, timeout_seconds: u64) {
    eprintln!(
        "[self-iterate] command timeout name={} exit={} duration_ms={} timeout_s={}",
        result.name, result.exit_code, result.duration_ms, timeout_seconds
    );
}

fn log_command_invalid(spec: &CommandSpec, started: Instant, message: &str) {
    eprintln!(
        "[self-iterate] command failed_to_start name={} duration_ms={} message={:?}",
        spec.name,
        started.elapsed().as_millis(),
        compact_log_text(message, 240)
    );
}

fn command_program(spec: &CommandSpec) -> &str {
    spec.command
        .first()
        .map(String::as_str)
        .unwrap_or("<empty>")
}

fn compact_log_text(value: &str, max_chars: usize) -> String {
    let normalized = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    tail(normalized.trim(), max_chars)
}

pub fn last_output_line(stdout: &str, stderr: &str) -> String {
    for output in [stderr, stdout] {
        if let Some(line) = output.lines().map(str::trim).rfind(|line| !line.is_empty()) {
            return tail(line, 400);
        }
    }
    String::new()
}

pub fn tail(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_owned();
    }
    value.chars().skip(count - max_chars).collect()
}

fn failed_result(
    spec: &CommandSpec,
    exit_code: i32,
    started: Instant,
    stderr: &str,
) -> CommandResult {
    CommandResult {
        name: spec.name.clone(),
        command: spec.command.clone(),
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        stdout: String::new(),
        stderr: stderr.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_stderr_last_line() {
        assert_eq!(last_output_line("ok\n", "warn\nerr\n"), "err");
    }

    #[test]
    fn reads_child_output_while_writing_stdin() {
        let input = "x".repeat(200_000);
        let result = run_command(
            &CommandSpec::new(
                "pipe_pressure",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "head -c 200000 /dev/zero; wc -c 1>&2".to_owned(),
                ],
                &std::env::current_dir().expect("current dir"),
                None,
                5,
            )
            .with_stdin(input),
        );

        assert!(result.passed(), "{}", result.gate_message());
        assert_eq!(result.stdout.len(), 200_000);
        assert_eq!(result.stderr.trim(), "200000");
    }
}
