pub fn git(workspace: &Path, args: &[&str], timeout_seconds: u64) -> CommandResult {
    let mut command = vec!["git".to_owned()];
    command.extend(args.iter().map(|arg| (*arg).to_owned()));
    run_command(&CommandSpec::new(
        "git",
        command,
        workspace,
        None,
        timeout_seconds,
    ))
}

pub fn git_checked(
    workspace: &Path,
    args: &[&str],
    timeout_seconds: u64,
) -> Result<CommandResult, String> {
    let result = git(workspace, args, timeout_seconds);
    if result.passed() {
        Ok(result)
    } else {
        Err(result.gate_message())
    }
}
