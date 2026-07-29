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
