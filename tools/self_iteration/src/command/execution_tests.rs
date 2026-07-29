use super::*;

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
