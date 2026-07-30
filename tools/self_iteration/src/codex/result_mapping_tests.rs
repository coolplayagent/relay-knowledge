use super::*;

#[test]
fn command_result_mapping_preserves_observable_fields() {
    let result = CommandResult {
        name: "codex_generation".to_owned(),
        command: vec!["codex".to_owned(), "exec".to_owned()],
        exit_code: 7,
        duration_ms: 55,
        stdout: "stdout".to_owned(),
        stderr: "stderr".to_owned(),
    };

    let mapped = from_command(result);

    assert_eq!(mapped.command, ["codex", "exec"]);
    assert_eq!(mapped.exit_code, 7);
    assert_eq!(mapped.duration_ms, 55);
    assert_eq!(mapped.stdout, "stdout");
    assert_eq!(mapped.stderr, "stderr");
}
