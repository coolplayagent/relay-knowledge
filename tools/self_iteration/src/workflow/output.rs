pub(crate) fn print_score(record: &serde_json::Value) {
    let score_accepted = record
        .get("score_accepted")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_else(|| record["accepted"].as_bool().unwrap_or(false));
    let status = if record["accepted"].as_bool().unwrap_or(false) {
        "accepted"
    } else if score_accepted {
        "would_accept"
    } else {
        "rejected"
    };
    println!(
        "[self-iterate] {status} score={:.6} foundational={:.6} competitive={:.6} accuracy={:.6} semantic_vector={:.6} research_judge={} performance={:.6} stability={:.6}",
        number(record, "score"),
        number(record, "foundational_capability"),
        number(record, "competitive_capability"),
        number(record, "accuracy"),
        number(record, "semantic_vector"),
        record
            .get("research_judge")
            .and_then(serde_json::Value::as_f64)
            .map(|value| format!("{value:.6}"))
            .unwrap_or_else(|| "n/a".to_owned()),
        number(record, "performance"),
        number(record, "stability"),
    );
    let reasons = record
        .get("reject_reasons")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !reasons.is_empty() {
        println!("[self-iterate] reasons: {}", reasons.join("; "));
    } else if status == "would_accept" {
        println!(
            "[self-iterate] reasons: score passed, but this mode does not create an accepted git commit"
        );
    }
    if let Some(baseline) = record.get("comparison_baseline") {
        let latest = baseline
            .get("latest_run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("none");
        let latest_score = baseline
            .get("latest_score")
            .and_then(serde_json::Value::as_f64)
            .map(|score| format!("{score:.6}"))
            .unwrap_or_else(|| "n/a".to_owned());
        let best = baseline
            .get("best_accepted_run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("none");
        let best_score = baseline
            .get("best_accepted_score")
            .and_then(serde_json::Value::as_f64)
            .map(|score| format!("{score:.6}"))
            .unwrap_or_else(|| "n/a".to_owned());
        let profile_best = baseline
            .get("profile_best_accepted_run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("none");
        let profile_best_score = baseline
            .get("profile_best_accepted_score")
            .and_then(serde_json::Value::as_f64)
            .map(|score| format!("{score:.6}"))
            .unwrap_or_else(|| "n/a".to_owned());
        println!(
            "[self-iterate] comparison baseline latest={latest} score={latest_score}; best_accepted={best} score={best_score}; profile_best_accepted={profile_best} score={profile_best_score}"
        );
    }
    println!(
        "[self-iterate] report: {}",
        record["report"].as_str().unwrap_or("")
    );
}

pub(crate) fn number(record: &serde_json::Value, name: &str) -> f64 {
    record
        .get(name)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
}
