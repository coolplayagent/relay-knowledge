use crate::config::Config;

use super::LayeredCycleOutcome;

pub(super) fn unattended_sleep_seconds(config: &Config, outcome: LayeredCycleOutcome) -> u64 {
    match outcome {
        LayeredCycleOutcome::Accepted => config.cooldown_after_accept_seconds,
        LayeredCycleOutcome::CodexTimeout => config.cooldown_after_timeout_seconds,
        LayeredCycleOutcome::Rejected
        | LayeredCycleOutcome::EmptyCandidate
        | LayeredCycleOutcome::CodexFailed => config.cycle_sleep_seconds,
    }
}

pub(super) fn score_accepted(record: &serde_json::Value) -> bool {
    record
        .get("score_accepted")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}
