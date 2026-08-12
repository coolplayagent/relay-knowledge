//! Direct tests for bounded Git child-process output and lifetime enforcement.

use std::{
    io::{Cursor, Write},
    process::Command,
    time::{Duration, Instant},
};

use super::*;

const FIXTURE_MODE_ENV: &str = "RELAY_KNOWLEDGE_BOUNDED_GIT_FIXTURE_MODE";
const FIXTURE_TEST_NAME: &str = "code::source::git::bounded::tests::bounded_subprocess_fixture";

#[test]
fn name_status_reader_counts_both_rename_paths() {
    let bytes = b"M\0src/lib.rs\0R100\0src/old.rs\0src/new.rs\0";

    let output = read_name_status_stdout(Cursor::new(bytes), 3, bytes.len())
        .expect("three touched paths should fit the exact budget");

    assert_eq!(output, bytes);
}

#[test]
fn name_status_reader_rejects_the_first_rename_path_past_the_limit() {
    let bytes = b"M\0a.rs\0R100\0old.rs\0new.rs\0";

    let error = read_name_status_stdout(Cursor::new(bytes), 2, bytes.len())
        .expect_err("the rename destination should exceed the path budget");

    assert!(matches!(
        error,
        StdoutReadError::ChangedPathLimit {
            observed: 3,
            limit: 2
        }
    ));
}

#[test]
fn name_status_reader_rejects_an_unterminated_oversized_token() {
    let bytes = vec![b'x'; 65];

    let error = read_name_status_stdout(Cursor::new(bytes), 2, 64)
        .expect_err("an unterminated token must still have a byte bound");

    assert!(matches!(error, StdoutReadError::ByteLimit { limit: 64 }));
}

#[test]
fn small_output_reader_accepts_the_exact_byte_limit() {
    let bytes = b"0123456789";

    let output = read_byte_bounded_stdout(Cursor::new(bytes), bytes.len())
        .expect("the exact byte budget should be accepted");

    assert_eq!(output, bytes);
}

#[test]
fn nul_record_reader_matches_without_buffering_following_output() {
    let bytes = b"100644 blob a\tfirst.rs\0\
160000 commit b\tvendor/module\0unterminated trailing output";

    let matched = read_nul_records_until_match(Cursor::new(bytes), 2, 64, is_gitlink_record)
        .expect("the matching second record should end the scan");

    assert!(matched);
}

#[test]
fn nul_record_reader_enforces_record_count_and_record_byte_limits() {
    let count_error =
        read_nul_records_until_match(Cursor::new(b"first\0second\0"), 1, 64, is_gitlink_record)
            .expect_err("the second record should exceed the count budget");
    assert!(matches!(
        count_error,
        NulRecordReadError::RecordLimit {
            observed: 2,
            limit: 1
        }
    ));

    let byte_error = read_nul_records_until_match(Cursor::new(b"12345\0"), 2, 4, is_gitlink_record)
        .expect_err("the record should exceed the byte budget");
    assert!(matches!(
        byte_error,
        NulRecordReadError::RecordByteLimit { limit: 4 }
    ));
}

#[test]
fn small_output_overflow_terminates_the_child_without_waiting_for_natural_exit() {
    let started = Instant::now();
    let error = run_small_output_command(
        fixture_command("bytes_then_sleep"),
        vec!["rev-parse".to_owned()],
        small_output_test_budget(64, 4096, Duration::from_secs(4)),
        "Git ref resolution",
    )
    .expect_err("the identity output budget should stop the child");

    assert!(error.to_string().contains("Git ref resolution output"));
    assert!(error.to_string().contains("bounded limit of 64 bytes"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "identity output overflow should terminate the fixture promptly"
    );
}

#[test]
fn small_output_timeout_terminates_the_child() {
    let started = Instant::now();
    let error = run_small_output_command(
        fixture_command("sleep"),
        vec!["rev-parse".to_owned()],
        small_output_test_budget(4096, 4096, Duration::from_millis(40)),
        "Git tree resolution",
    )
    .expect_err("the identity deadline should stop the child");

    assert!(
        error
            .to_string()
            .contains("Git tree resolution timed out after 40 ms")
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "identity timeout should terminate the fixture promptly"
    );
}

#[test]
fn nul_record_match_terminates_the_child_without_waiting_for_natural_exit() {
    let started = Instant::now();
    let matched = run_nul_record_match_command(
        fixture_command("gitlink_then_sleep"),
        vec!["ls-tree".to_owned()],
        nul_record_test_budget(8, 4096, 4096, Duration::from_secs(4)),
        is_gitlink_record,
        "Git tree gitlink probe",
    )
    .expect("the streamed gitlink should match");

    assert!(matched);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "a matching record should terminate the sleeping fixture promptly"
    );
}

#[test]
fn nul_record_probe_timeout_terminates_the_child() {
    let error = run_nul_record_match_command(
        fixture_command("sleep"),
        vec!["ls-tree".to_owned()],
        nul_record_test_budget(8, 4096, 4096, Duration::from_millis(40)),
        is_gitlink_record,
        "Git tree gitlink probe",
    )
    .expect_err("the NUL-record deadline should stop the child");

    assert!(
        error
            .to_string()
            .contains("Git tree gitlink probe timed out after 40 ms")
    );
}

#[test]
fn changed_path_overflow_terminates_the_child_without_waiting_for_natural_exit() {
    let started = Instant::now();
    let error = run_name_status_command(
        fixture_command("changes_then_sleep"),
        vec!["diff".to_owned()],
        test_budget(2, 4096, 4096, Duration::from_secs(4)),
    )
    .expect_err("the third streamed change should stop the child");

    assert!(error.to_string().contains("reached 3 changed paths"));
    assert!(error.to_string().contains("run a full code index"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "overflow should terminate the sleeping fixture promptly"
    );
}

#[test]
fn stdout_byte_overflow_terminates_the_child_without_waiting_for_natural_exit() {
    let started = Instant::now();
    let error = run_name_status_command(
        fixture_command("bytes_then_sleep"),
        vec!["diff".to_owned()],
        test_budget(8, 64, 4096, Duration::from_secs(4)),
    )
    .expect_err("the byte budget should stop the child");

    assert!(error.to_string().contains("bounded limit of 64 bytes"));
    assert!(error.to_string().contains("run a full code index"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "byte overflow should terminate the sleeping fixture promptly"
    );
}

#[test]
fn timeout_terminates_the_child_and_guides_the_caller_to_full_index() {
    let started = Instant::now();
    let error = run_name_status_command(
        fixture_command("sleep"),
        vec!["diff".to_owned()],
        test_budget(8, 4096, 4096, Duration::from_millis(40)),
    )
    .expect_err("the deadline should stop the child");

    assert!(error.to_string().contains("timed out after 40 ms"));
    assert!(error.to_string().contains("run a full code index"));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "timeout should terminate the sleeping fixture promptly"
    );
}

#[test]
fn failed_command_drains_but_bounds_stderr() {
    let error = run_name_status_command(
        fixture_command("stderr_failure"),
        vec!["diff".to_owned()],
        test_budget(8, 4096, 128, Duration::from_secs(4)),
    )
    .expect_err("the failing fixture should map bounded stderr");

    let rendered = error.to_string();
    assert!(rendered.contains("stderr truncated to 128 bytes"));
    assert!(rendered.len() < 512, "stderr must not grow without bound");
}

fn fixture_command(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable should exist"));
    command
        .args(["--ignored", "--exact", FIXTURE_TEST_NAME, "--nocapture"])
        .env(FIXTURE_MODE_ENV, mode);
    command
}

fn test_budget(
    max_paths: usize,
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    timeout: Duration,
) -> GitNameStatusBudget {
    GitNameStatusBudget {
        max_paths,
        max_stdout_bytes,
        max_stderr_bytes,
        timeout,
    }
}

fn small_output_test_budget(
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    timeout: Duration,
) -> GitSmallOutputBudget {
    GitSmallOutputBudget {
        max_stdout_bytes,
        max_stderr_bytes,
        timeout,
    }
}

fn nul_record_test_budget(
    max_records: usize,
    max_record_bytes: usize,
    max_stderr_bytes: usize,
    timeout: Duration,
) -> GitNulRecordBudget {
    GitNulRecordBudget {
        max_records,
        max_record_bytes,
        max_stderr_bytes,
        timeout,
    }
}

fn is_gitlink_record(record: &[u8]) -> bool {
    record.starts_with(b"160000 ")
}

#[test]
#[ignore = "subprocess fixture invoked by bounded Git tests"]
fn bounded_subprocess_fixture() {
    match std::env::var(FIXTURE_MODE_ENV).as_deref() {
        Ok("changes_then_sleep") => {
            std::io::stdout()
                .write_all(b"M\0a.rs\0A\0b.rs\0D\0c.rs\0")
                .expect("fixture stdout should write");
            std::io::stdout()
                .flush()
                .expect("fixture stdout should flush");
            std::thread::sleep(Duration::from_secs(5));
        }
        Ok("bytes_then_sleep") => {
            std::io::stdout()
                .write_all(&vec![b'x'; 4096])
                .expect("fixture stdout should write");
            std::io::stdout()
                .flush()
                .expect("fixture stdout should flush");
            std::thread::sleep(Duration::from_secs(5));
        }
        Ok("gitlink_then_sleep") => {
            std::io::stdout()
                .write_all(
                    b"100644 blob a\tfirst.rs\0\
160000 commit b\tvendor/module\0",
                )
                .expect("fixture stdout should write");
            std::io::stdout()
                .flush()
                .expect("fixture stdout should flush");
            std::thread::sleep(Duration::from_secs(5));
        }
        Ok("sleep") => std::thread::sleep(Duration::from_secs(5)),
        Ok("stderr_failure") => {
            std::io::stderr()
                .write_all(&vec![b'e'; 4096])
                .expect("fixture stderr should write");
            std::io::stderr()
                .flush()
                .expect("fixture stderr should flush");
            panic!("intentional bounded stderr fixture failure");
        }
        other => panic!("unexpected bounded subprocess fixture mode: {other:?}"),
    }
}
