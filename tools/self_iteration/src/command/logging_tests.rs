use super::*;

fn spec(command: Vec<String>) -> CommandSpec {
    CommandSpec::new(
        "fixture",
        command,
        &std::env::current_dir().expect("current directory should resolve"),
        None,
        1,
    )
}

#[test]
fn command_program_handles_present_and_empty_commands() {
    assert_eq!(command_program(&spec(vec!["cargo".to_owned()])), "cargo");
    assert_eq!(command_program(&spec(Vec::new())), "<empty>");
}

#[test]
fn compact_log_text_removes_controls_and_keeps_the_bounded_tail() {
    assert_eq!(
        compact_log_text("first\nsecond\tthird", 40),
        "first second third"
    );
    assert_eq!(compact_log_text("0123456789", 4), "6789");
}
