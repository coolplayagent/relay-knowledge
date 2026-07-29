use super::*;

#[test]
fn failed_result_preserves_command_identity_and_error() {
    let cwd = std::env::current_dir().expect("current directory should resolve");
    let spec = CommandSpec::new("missing", vec!["missing-program".to_owned()], &cwd, None, 3);

    let result = failed_result(&spec, 127, Instant::now(), "program not found");

    assert_eq!(result.name, "missing");
    assert_eq!(result.command, ["missing-program"]);
    assert_eq!(result.exit_code, 127);
    assert!(result.stdout.is_empty());
    assert_eq!(result.stderr, "program not found");
}
