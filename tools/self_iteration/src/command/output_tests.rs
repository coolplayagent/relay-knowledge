use super::*;

#[test]
fn prefers_stderr_last_line() {
    assert_eq!(last_output_line("ok\n", "warn\nerr\n"), "err");
}
