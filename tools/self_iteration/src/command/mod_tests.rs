use super::*;

#[test]
fn command_result_exposes_status_message_and_bounded_serialization() {
    let result = CommandResult {
        name: "fixture".to_owned(),
        command: vec!["fixture".to_owned(), "--flag".to_owned()],
        exit_code: 2,
        duration_ms: 17,
        stdout: format!("prefix\n{}", "o".repeat(4_100)),
        stderr: "first error\nlast error\n".to_owned(),
    };

    assert!(!result.passed());
    assert_eq!(result.gate_message(), "last error");
    let serialized = result.serializable();
    assert_eq!(serialized["name"], "fixture");
    assert_eq!(serialized["exit_code"], 2);
    assert_eq!(
        serialized["stdout_tail"]
            .as_str()
            .expect("stdout tail should be a string")
            .chars()
            .count(),
        4_000
    );
}

#[test]
fn command_spec_preserves_execution_boundaries_and_stdin() {
    let cwd = std::env::current_dir().expect("current directory should resolve");
    let mut env = BTreeMap::new();
    env.insert("FIXTURE".to_owned(), "value".to_owned());

    let spec = CommandSpec::new(
        "fixture",
        vec!["program".to_owned(), "argument".to_owned()],
        &cwd,
        Some(env.clone()),
        9,
    )
    .with_stdin("input".to_owned());

    assert_eq!(spec.name, "fixture");
    assert_eq!(spec.command, ["program", "argument"]);
    assert_eq!(spec.cwd, cwd);
    assert_eq!(spec.env, Some(env));
    assert_eq!(spec.timeout_seconds, 9);
    assert_eq!(spec.stdin.as_deref(), Some("input"));
}
