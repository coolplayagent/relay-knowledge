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
