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

#[test]
fn short_lived_children_do_not_accumulate_a_polling_quantum() {
    const CHILD_COUNT: usize = 16;
    const AGGREGATE_DURATION_LIMIT_MS: u64 = 240;

    let cwd = std::env::current_dir().expect("current dir");
    let mut aggregate_duration_ms = 0u64;
    for index in 0..CHILD_COUNT {
        let result = run_command(&CommandSpec::new(
            format!("short_child_{index}"),
            vec!["sh".to_owned(), "-c".to_owned(), "sleep 0.002".to_owned()],
            &cwd,
            None,
            5,
        ));
        assert!(result.passed(), "{}", result.gate_message());
        aggregate_duration_ms = aggregate_duration_ms.saturating_add(result.duration_ms);
    }

    assert!(
        aggregate_duration_ms < AGGREGATE_DURATION_LIMIT_MS,
        "{CHILD_COUNT} short children accumulated {aggregate_duration_ms} ms; command completion \
         appears to be waiting on a fixed polling quantum"
    );
}
