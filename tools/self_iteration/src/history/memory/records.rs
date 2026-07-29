
fn primary_memory(record: &Value) -> Value {
    let kind = primary_kind(record);
    memory_payload(
        record,
        &kind,
        &primary_title(&kind, record),
        &primary_summary(&kind, record),
        None,
    )
}

fn regression_memory(record: &Value) -> Option<Value> {
    let degradations = record.get("degradations")?.as_array()?;
    if degradations.is_empty() {
        return None;
    }
    let first = &degradations[0];
    let objective = first
        .get("objective")
        .and_then(Value::as_str)
        .unwrap_or("performance");
    let kind = match objective {
        "semantic_vector" => "semantic_vector_regression",
        "research_judge" => "research_judge_regression",
        "competitive_capability" => "competitive_capability_regression",
        "foundational_capability" => "foundational_capability_regression",
        _ => "performance_regression",
    };
    let name = first
        .get("name")
        .or_else(|| first.get("case_id"))
        .and_then(Value::as_str)
        .unwrap_or("regression");
    Some(memory_payload(
        record,
        kind,
        &format!(
            "{} recorded {} regression",
            string_field(record, "run_id"),
            name
        ),
        &format!(
            "Run {} recorded a {} while scoring {}. Future iterations should inspect detail before related changes.",
            string_field(record, "run_id"),
            kind.replace('_', " "),
            record
                .get("score")
                .map(Value::to_string)
                .unwrap_or_default()
        ),
        Some(name),
    ))
}

fn repeated_rejection_cluster_memory(
    paths: &history::HistoryPaths,
    record: &Value,
) -> Option<Value> {
    if history::adopted(record) {
        return None;
    }
    let reason = primary_reject_reason(record)?;
    let mut run_ids = vec![string_field(record, "run_id")];
    let runs = history::load_runs(paths).unwrap_or_default();
    for previous in runs.iter().rev() {
        if history::adopted(previous) {
            break;
        }
        if primary_reject_reason(previous).as_deref() != Some(reason.as_str()) {
            break;
        }
        run_ids.push(string_field(previous, "run_id"));
    }
    if run_ids.len() < 2 {
        return None;
    }
    let summary = format!(
        "Run {} extends a consecutive rejection cluster for reason `{}` across {} run(s): {}. Changed paths: {}. Latest improvements: {}. Latest degradations: {}. Future iterations should choose a different strategy or directly address this cluster before retrying related files.",
        string_field(record, "run_id"),
        reason,
        run_ids.len(),
        run_ids.join(", "),
        compact_paths(record, 5),
        top_change_summary(record, "improvements", 5),
        top_change_summary(record, "degradations", 5)
    );
    Some(memory_payload(
        record,
        "repeated_rejection_cluster",
        &format!(
            "{} repeated rejection cluster: {}",
            string_field(record, "run_id"),
            compact_prompt_text(&reason, 90)
        ),
        &summary,
        Some(&reason),
    ))
}

fn memory_payload(
    record: &Value,
    kind: &str,
    title: &str,
    summary: &str,
    suffix: Option<&str>,
) -> Value {
    let run_id = string_field(record, "run_id");
    let id = safe_id(&format!(
        "{}-{}{}",
        run_id,
        kind,
        suffix
            .map(|value| format!("-{}", safe_id(value)))
            .unwrap_or_default()
    ));
    serde_json::json!({
        "id": id,
        "run_id": run_id,
        "kind": kind,
        "title": title,
        "summary": summary,
        "detail": run_detail(summary, record),
        "tags": run_tags(record, kind),
        "paths": related_paths(record),
        "score_impact": score_impact(record),
        "created_at": string_field(record, "timestamp"),
    })
}

fn write_memory_item(paths: &history::HistoryPaths, item: &Value) -> Result<Value, String> {
    let item_id = string_field(item, "id");
    let summary_path = paths.memory_summaries.join(format!("{item_id}.md"));
    let detail_path = paths.memory_details.join(format!("{item_id}.md"));
    fs::write(&summary_path, memory_markdown(item, "summary"))
        .map_err(|error| format!("failed to write {}: {error}", summary_path.display()))?;
    fs::write(&detail_path, memory_markdown(item, "detail"))
        .map_err(|error| format!("failed to write {}: {error}", detail_path.display()))?;
    Ok(serde_json::json!({
        "id": item_id,
        "run_id": item["run_id"],
        "kind": item["kind"],
        "title": item["title"],
        "tags": item["tags"],
        "paths": item["paths"],
        "score_impact": item["score_impact"],
        "created_at": item["created_at"],
        "summary_path": summary_path.display().to_string(),
        "detail_path": detail_path.display().to_string(),
    }))
}
