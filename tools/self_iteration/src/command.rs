use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

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
        return failed_result(spec, 1, started, "empty command");
    };
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
        Err(error) => return failed_result(spec, 1, started, &error.to_string()),
    };
    if let Some(stdin) = &spec.stdin {
        if let Some(mut handle) = child.stdin.take() {
            use std::io::Write;
            if let Err(error) = handle.write_all(stdin.as_bytes()) {
                return failed_result(spec, 1, started, &error.to_string());
            }
        }
    }
    let stdout_reader = child.stdout.take().map(read_pipe);
    let stderr_reader = child.stderr.take().map(read_pipe);
    let timeout = Duration::from_secs(spec.timeout_seconds);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return CommandResult {
                    name: spec.name.clone(),
                    command: spec.command.clone(),
                    exit_code: status.code().unwrap_or(1),
                    duration_ms: started.elapsed().as_millis() as u64,
                    stdout: join_reader(stdout_reader),
                    stderr: join_reader(stderr_reader),
                };
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let stdout = join_reader(stdout_reader);
                let mut stderr = join_reader(stderr_reader);
                stderr.push_str(&format!("\ntimeout after {}s", spec.timeout_seconds));
                return CommandResult {
                    name: spec.name.clone(),
                    command: spec.command.clone(),
                    exit_code: 124,
                    duration_ms: started.elapsed().as_millis() as u64,
                    stdout,
                    stderr,
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return failed_result(spec, 1, started, &error.to_string()),
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

fn join_reader(reader: Option<JoinHandle<String>>) -> String {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
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
}
