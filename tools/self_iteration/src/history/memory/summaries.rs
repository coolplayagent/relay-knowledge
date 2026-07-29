fn primary_reject_reason(record: &Value) -> Option<String> {
    string_array(record, "reject_reasons")
        .into_iter()
        .find(|reason| !reason.trim().is_empty())
}

fn protected_floor_summary(record: &Value) -> String {
    format!(
        "foundational={}, competitive={}, semantic_vector={}, stability={}",
        field_string(record, "foundational_capability"),
        field_string(record, "competitive_capability"),
        field_string(record, "semantic_vector"),
        field_string(record, "stability")
    )
}

fn compact_paths(record: &Value, limit: usize) -> String {
    let paths = changed_paths(record);
    if paths.is_empty() {
        return "none recorded".to_owned();
    }
    let omitted = paths.len().saturating_sub(limit);
    let mut selected = paths.into_iter().take(limit).collect::<Vec<_>>();
    if omitted > 0 {
        selected.push(format!("+{omitted} more"));
    }
    selected.join(", ")
}

fn top_change_summary(record: &Value, field: &str, limit: usize) -> String {
    let changes = compact_score_changes(value_array(record, field), limit);
    if changes.is_empty() {
        "none recorded".to_owned()
    } else {
        changes.join("; ")
    }
}

fn score_delta_summary(record: &Value) -> String {
    value_array(record, "improvements")
        .iter()
        .chain(value_array(record, "degradations"))
        .find(|change| {
            change.get("kind").and_then(Value::as_str) == Some("score_component")
                && change.get("name").and_then(Value::as_str) == Some("score")
        })
        .and_then(|change| {
            Some(format!(
                "{:+.6}",
                change.get("current")?.as_f64()? - change.get("previous")?.as_f64()?
            ))
        })
        .unwrap_or_else(|| "within epsilon or unavailable".to_owned())
}

fn primary_kind(record: &Value) -> String {
    if history::adopted(record) {
        "accepted_optimization".to_owned()
    } else if !failed_gate_names(record).is_empty() {
        "quality_gate_failure".to_owned()
    } else {
        "rejected_attempt".to_owned()
    }
}

fn primary_title(kind: &str, record: &Value) -> String {
    let run_id = string_field(record, "run_id");
    if kind == "accepted_optimization" {
        format!("{run_id} accepted optimization")
    } else if kind == "quality_gate_failure" {
        format!(
            "{run_id} failed gates: {}",
            failed_gate_names(record).join(", ")
        )
    } else {
        format!("{run_id} rejected attempt")
    }
}

fn primary_summary(kind: &str, record: &Value) -> String {
    let run_id = string_field(record, "run_id");
    if kind == "accepted_optimization" {
        format!(
            "Accepted run {} scored {}. Protected floors: {}. Changed paths: {}. Key improvements: {}. Known degradations: {}.",
            run_id,
            record
                .get("score")
                .map(Value::to_string)
                .unwrap_or_default(),
            protected_floor_summary(record),
            compact_paths(record, 6),
            top_change_summary(record, "improvements", 6),
            top_change_summary(record, "degradations", 4)
        )
    } else if kind == "quality_gate_failure" {
        format!(
            "Rejected run {} failed quality gates {}. Changed paths: {}. Top improvements: {}. Top degradations: {}. Inspect detail before retrying related changes.",
            run_id,
            failed_gate_names(record).join(", "),
            compact_paths(record, 6),
            top_change_summary(record, "improvements", 4),
            top_change_summary(record, "degradations", 6)
        )
    } else {
        format!(
            "Rejected run {} scored {}. Score delta: {}. Reasons: {}. Changed paths: {}. Top improvements: {}. Top degradations: {}.",
            run_id,
            record
                .get("score")
                .map(Value::to_string)
                .unwrap_or_default(),
            score_delta_summary(record),
            string_array(record, "reject_reasons").join("; "),
            compact_paths(record, 6),
            top_change_summary(record, "improvements", 6),
            top_change_summary(record, "degradations", 6)
        )
    }
}

fn run_detail(summary: &str, record: &Value) -> String {
    [
        summary.to_owned(),
        format!(
            "## Score\n\n- score: {}\n- foundational_capability: {}\n- competitive_capability: {}\n- accuracy: {}\n- semantic_vector: {}\n- research_judge: {}\n- performance: {}\n- stability: {}",
            field_string(record, "score"),
            field_string(record, "foundational_capability"),
            field_string(record, "competitive_capability"),
            field_string(record, "accuracy"),
            field_string(record, "semantic_vector"),
            field_string(record, "research_judge"),
            field_string(record, "performance"),
            field_string(record, "stability"),
        ),
        markdown_list("Changed Paths", &changed_paths(record)),
        markdown_list("Reject Reasons", &string_array(record, "reject_reasons")),
        markdown_list("Improvements", &compact_score_changes(value_array(record, "improvements"), 12)),
        markdown_list("Degradations", &compact_score_changes(value_array(record, "degradations"), 12)),
        markdown_list("Failed Gates", &failed_gate_names(record)),
        markdown_list("Key Metrics", &key_metric_lines(record)),
        markdown_list("Case Signals", &case_signal_lines(record)),
    ]
    .join("\n\n")
}
