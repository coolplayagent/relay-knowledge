use std::time::{Duration, Instant};

use super::{CommandResult, CommandSpec, output::tail};

pub(super) fn log_command_started(spec: &CommandSpec) {
    eprintln!(
        "[self-iterate] command start name={} program={} argc={} timeout_s={}",
        spec.name,
        compact_log_text(command_program(spec), 120),
        spec.command.len(),
        spec.timeout_seconds
    );
}

pub(super) fn log_command_running(spec: &CommandSpec, elapsed: Duration) {
    eprintln!(
        "[self-iterate] command running name={} elapsed_s={} timeout_s={}",
        spec.name,
        elapsed.as_secs(),
        spec.timeout_seconds
    );
}

pub(super) fn log_command_finished(result: &CommandResult) {
    let status = if result.passed() { "ok" } else { "failed" };
    let message = result.gate_message();
    if result.passed() || message.is_empty() {
        eprintln!(
            "[self-iterate] command done name={} status={} exit={} duration_ms={}",
            result.name, status, result.exit_code, result.duration_ms
        );
    } else {
        eprintln!(
            "[self-iterate] command done name={} status={} exit={} duration_ms={} message={:?}",
            result.name,
            status,
            result.exit_code,
            result.duration_ms,
            compact_log_text(&message, 240)
        );
    }
}

pub(super) fn log_command_timeout(result: &CommandResult, timeout_seconds: u64) {
    eprintln!(
        "[self-iterate] command timeout name={} exit={} duration_ms={} timeout_s={}",
        result.name, result.exit_code, result.duration_ms, timeout_seconds
    );
}

pub(super) fn log_command_invalid(spec: &CommandSpec, started: Instant, message: &str) {
    eprintln!(
        "[self-iterate] command failed_to_start name={} duration_ms={} message={:?}",
        spec.name,
        started.elapsed().as_millis(),
        compact_log_text(message, 240)
    );
}

fn command_program(spec: &CommandSpec) -> &str {
    spec.command
        .first()
        .map(String::as_str)
        .unwrap_or("<empty>")
}

fn compact_log_text(value: &str, max_chars: usize) -> String {
    let normalized = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    tail(normalized.trim(), max_chars)
}

#[cfg(test)]
#[path = "logging_tests.rs"]
mod logging_tests;
