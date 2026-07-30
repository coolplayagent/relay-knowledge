use serde_json::Value;

pub(super) fn committed(run: &Value) -> bool {
    run.get("committed")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            run.get("commit")
                .and_then(Value::as_str)
                .is_some_and(|commit| !commit.trim().is_empty())
        })
}

pub fn adopted(run: &Value) -> bool {
    committed(run)
}

pub(super) fn score_accepted(run: &Value) -> bool {
    run.get("score_accepted")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            run.get("accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
}

pub(super) fn run_mode(run: &Value) -> String {
    if is_evaluate_run(run) {
        "evaluate".to_owned()
    } else {
        "loop".to_owned()
    }
}

pub fn is_evaluate_run(run: &Value) -> bool {
    run.get("run_id")
        .and_then(Value::as_str)
        .is_some_and(|run_id| run_id.starts_with("manual-evaluate"))
}

pub(super) fn automated_baseline_run(run: &Value) -> bool {
    !is_evaluate_run(run) && !is_no_diff_run(run)
}

fn is_no_diff_run(run: &Value) -> bool {
    run.get("generated_diff").and_then(Value::as_bool) == Some(false)
        || run
            .get("reject_reasons")
            .and_then(Value::as_array)
            .is_some_and(|reasons| {
                reasons
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|reason| reason.contains("no candidate diff"))
            })
}

pub(super) fn adoption_status(committed: bool, score_accepted: bool) -> &'static str {
    if committed {
        "committed"
    } else if score_accepted {
        "would_accept"
    } else {
        "rejected"
    }
}

pub(super) fn adoption_status_for_run(run: &Value) -> String {
    run.get("adoption_status")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| adoption_status(committed(run), score_accepted(run)).to_owned())
}

#[cfg(test)]
#[path = "run_state_tests.rs"]
mod run_state_tests;
