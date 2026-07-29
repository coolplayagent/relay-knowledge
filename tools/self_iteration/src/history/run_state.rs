fn reject_reasons(run: &Value) -> String {
    run.get("reject_reasons")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn committed(run: &Value) -> bool {
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

fn score_accepted(run: &Value) -> bool {
    run.get("score_accepted")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            run.get("accepted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
}

fn run_mode(run: &Value) -> String {
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

fn automated_baseline_run(run: &Value) -> bool {
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

fn adoption_status(committed: bool, score_accepted: bool) -> &'static str {
    if committed {
        "committed"
    } else if score_accepted {
        "would_accept"
    } else {
        "rejected"
    }
}

fn adoption_status_for_run(run: &Value) -> String {
    run.get("adoption_status")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| adoption_status(committed(run), score_accepted(run)).to_owned())
}

fn patch_string(run: &Value, name: &str) -> String {
    run.get("patch")
        .and_then(|patch| patch.get(name))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn patch_number(run: &Value, name: &str) -> u64 {
    run.get("patch")
        .and_then(|patch| patch.get(name))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn rounded(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn csv(run: &Value, name: &str) -> String {
    escape_csv(run.get(name).and_then(Value::as_str).unwrap_or(""))
}

fn number(run: &Value, name: &str) -> f64 {
    run.get(name).and_then(Value::as_f64).unwrap_or(0.0)
}

fn optional_number(run: &Value, name: &str) -> String {
    run.get(name)
        .and_then(Value::as_f64)
        .map(|value| value.to_string())
        .unwrap_or_default()
}

fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
