use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{git_ops::changed_paths_from_diff, history};

pub fn write_run_memory(paths: &history::HistoryPaths, record: &Value) -> Result<(), String> {
    paths.ensure()?;
    let mut items = vec![primary_memory(record)];
    if let Some(regression) = regression_memory(record) {
        items.push(regression);
    }
    if let Some(cluster) = repeated_rejection_cluster_memory(paths, record) {
        items.push(cluster);
    }
    let mut index = load_memory_index(paths);
    for item in items {
        let index_item = write_memory_item(paths, &item)?;
        index.retain(|existing| existing.get("id") != index_item.get("id"));
        index.push(index_item);
    }
    write_memory_index(paths, &index)
}

pub fn progressive_memory_index(paths: &history::HistoryPaths, limit: usize) -> String {
    let items = sorted_memory_items(paths);
    if items.is_empty() {
        return "No progressive memory entries recorded yet.".to_owned();
    }
    let mut lines = vec![
        "Use this as an index, not as full context. Read summary_path first, then detail_path only when relevant.".to_owned(),
    ];
    for item in items.iter().take(limit) {
        let tags = item
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(Value::as_str)
                    .take(8)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "none".to_owned());
        lines.push(format!(
            "- id={} kind={} title={} tags={} summary_path={} detail_path={}",
            string_field(item, "id"),
            string_field(item, "kind"),
            compact_prompt_text(&string_field(item, "title"), 180),
            tags,
            string_field(item, "summary_path"),
            string_field(item, "detail_path")
        ));
    }
    if items.len() > limit {
        lines.push(format!(
            "- {} older memory item(s) omitted from the prompt index.",
            items.len() - limit
        ));
    }
    lines.join("\n")
}

pub fn rejection_recovery_memory_review(paths: &history::HistoryPaths, limit: usize) -> String {
    let Ok(Some(latest)) = history::previous_scored_run(paths) else {
        return "No scored self-iteration run yet; no rejection recovery memory review required."
            .to_owned();
    };
    if history::adopted(&latest) {
        return "Latest scored run was accepted; no rejection recovery memory review required."
            .to_owned();
    }
    let items = sorted_memory_items(paths);
    if items.is_empty() {
        return format!(
            "Latest scored run {} was rejected, but no progressive memory entries are recorded yet.",
            string_field(&latest, "run_id")
        );
    }
    let mut lines = vec![format!(
        "Rejected recovery mode is active because latest scored run {} was rejected. Read summary_path for 3 to {} recent memory entries when available; open detail_path or patch files only for entries matching the current rejection reason, gate, case, metric, path, or algorithm.",
        string_field(&latest, "run_id"),
        limit
    )];
    for item in items.iter().take(limit) {
        lines.push(format!(
            "- id={} run_id={} kind={} title={} summary_path={} detail_path={}",
            string_field(item, "id"),
            string_field(item, "run_id"),
            string_field(item, "kind"),
            compact_prompt_text(&string_field(item, "title"), 160),
            string_field(item, "summary_path"),
            string_field(item, "detail_path")
        ));
    }
    lines.join("\n")
}

pub fn historical_patch_memory_index(paths: &history::HistoryPaths, limit: usize) -> String {
    if !paths.patches.exists() {
        return "No historical patch files recorded yet.".to_owned();
    }
    let mut patch_files = fs::read_dir(&paths.patches)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("patch"))
        .collect::<Vec<_>>();
    patch_files.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    if patch_files.is_empty() {
        return "No historical patch files recorded yet.".to_owned();
    }
    let runs = history::load_runs(paths).unwrap_or_default();
    let mut lines = vec![
        "Use this as an index, not as full context. Read only patches that look relevant."
            .to_owned(),
    ];
    for patch_path in patch_files.iter().take(limit) {
        let run = runs.iter().find(|run| {
            run.get("patch")
                .and_then(|patch| patch.get("path"))
                .and_then(Value::as_str)
                .map(|path| Path::new(path).file_name() == patch_path.file_name())
                .unwrap_or(false)
        });
        let changed_paths = patch_changed_paths(patch_path, run);
        let status = run
            .map(|run| {
                if history::adopted(run) {
                    "committed"
                } else if run
                    .get("score_accepted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "would_accept"
                } else {
                    "rejected"
                }
            })
            .unwrap_or("unscored");
        let size = patch_path
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        lines.push(format!(
            "- patch={} size_bytes={} status={} score={} changed_paths={}",
            patch_path.display(),
            size,
            status,
            run.and_then(|run| run.get("score"))
                .map(Value::to_string)
                .unwrap_or_default(),
            changed_paths
                .iter()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if patch_files.len() > limit {
        lines.push(format!(
            "- {} older patch file(s) omitted from the prompt index.",
            patch_files.len() - limit
        ));
    }
    lines.join("\n")
}

pub fn compact_prompt_text(value: &str, limit: usize) -> String {
    let compact = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.len() <= limit {
        return compact;
    }
    compact
        .chars()
        .rev()
        .take(limit)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

pub fn compact_score_changes(changes: &[Value], limit: usize) -> Vec<String> {
    changes
        .iter()
        .take(limit)
        .filter_map(Value::as_object)
        .map(|change| {
            let name = change
                .get("name")
                .or_else(|| change.get("case_id"))
                .or_else(|| change.get("kind"))
                .map(Value::to_string)
                .unwrap_or_default();
            format!(
                "{}:{} {}->{} {}",
                change.get("kind").and_then(Value::as_str).unwrap_or(""),
                name.trim_matches('"'),
                change
                    .get("previous")
                    .map(Value::to_string)
                    .unwrap_or_default(),
                change
                    .get("current")
                    .map(Value::to_string)
                    .unwrap_or_default(),
                change
                    .get("reason")
                    .or_else(|| change.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
            )
            .trim()
            .to_owned()
        })
        .collect()
}

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

fn memory_markdown(item: &Value, body_key: &str) -> String {
    let tags = item
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "none".to_owned());
    let related = item
        .get("paths")
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(|path| format!("- `{path}`"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "- none".to_owned());
    format!(
        "# {}\n\n- id: `{}`\n- kind: `{}`\n- run: `{}`\n- tags: {}\n\n{}\n\n## Related Paths\n\n{}\n",
        string_field(item, "title"),
        string_field(item, "id"),
        string_field(item, "kind"),
        string_field(item, "run_id"),
        tags,
        item.get(body_key).and_then(Value::as_str).unwrap_or(""),
        related
    )
}

fn load_memory_index(paths: &history::HistoryPaths) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(&paths.memory_index) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(Value::is_object)
        .collect()
}

fn write_memory_index(paths: &history::HistoryPaths, items: &[Value]) -> Result<(), String> {
    let temp = paths.memory_index.with_extension("jsonl.tmp");
    let mut file = fs::File::create(&temp)
        .map_err(|error| format!("failed to write {}: {error}", temp.display()))?;
    for item in items {
        writeln!(
            file,
            "{}",
            serde_json::to_string(item).map_err(|error| error.to_string())?
        )
        .map_err(|error| format!("failed to write {}: {error}", temp.display()))?;
    }
    fs::rename(&temp, &paths.memory_index).map_err(|error| {
        format!(
            "failed to replace {}: {error}",
            paths.memory_index.display()
        )
    })
}

fn sorted_memory_items(paths: &history::HistoryPaths) -> Vec<Value> {
    let mut items = load_memory_index(paths);
    items.retain(|item| {
        !item
            .get("run_id")
            .and_then(Value::as_str)
            .is_some_and(|run_id| run_id.starts_with("manual-evaluate"))
    });
    items.sort_by(|left, right| {
        string_field(right, "created_at").cmp(&string_field(left, "created_at"))
    });
    items
}

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

fn markdown_list(title: &str, values: &[String]) -> String {
    let body = if values.is_empty() {
        "- none recorded".to_owned()
    } else {
        values
            .iter()
            .map(|value| format!("- {value}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("## {title}\n\n{body}")
}

fn changed_paths(record: &Value) -> Vec<String> {
    if let Some(paths) = record
        .get("optimization_plan")
        .and_then(|plan| plan.get("changed_paths"))
        .and_then(Value::as_array)
    {
        return paths
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
    }
    record
        .get("patch")
        .and_then(|patch| patch.get("path"))
        .and_then(Value::as_str)
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|diff| changed_paths_from_diff(&diff))
        .unwrap_or_default()
}

fn related_paths(record: &Value) -> Vec<String> {
    [
        record
            .get("patch")
            .and_then(|patch| patch.get("path"))
            .and_then(Value::as_str),
        record.get("report").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .map(ToOwned::to_owned)
    .collect()
}

fn score_impact(record: &Value) -> Value {
    serde_json::json!({
        "accepted": history::adopted(record),
        "score_accepted": record.get("score_accepted").cloned().unwrap_or(Value::Null),
        "score": record.get("score").cloned().unwrap_or(Value::Null),
        "foundational_capability": record.get("foundational_capability").cloned().unwrap_or(Value::Null),
        "competitive_capability": record.get("competitive_capability").cloned().unwrap_or(Value::Null),
        "semantic_vector": record.get("semantic_vector").cloned().unwrap_or(Value::Null),
        "research_judge": record.get("research_judge").cloned().unwrap_or(Value::Null),
        "performance": record.get("performance").cloned().unwrap_or(Value::Null),
        "stability": record.get("stability").cloned().unwrap_or(Value::Null),
        "improvement_count": value_array(record, "improvements").len(),
        "degradation_count": value_array(record, "degradations").len(),
    })
}

fn run_tags(record: &Value, kind: &str) -> Vec<String> {
    let mut tags = BTreeSet::from([
        safe_tag(kind),
        if history::adopted(record) {
            "accepted".to_owned()
        } else {
            "rejected".to_owned()
        },
    ]);
    for path in changed_paths(record).into_iter().take(8) {
        tags.insert(safe_tag(&path));
    }
    for gate in failed_gate_names(record).into_iter().take(4) {
        tags.insert(safe_tag(&gate));
    }
    tags.into_iter().collect()
}

fn failed_gate_names(record: &Value) -> Vec<String> {
    value_array(record, "gates")
        .iter()
        .filter(|gate| !gate.get("passed").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|gate| gate.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn key_metric_lines(record: &Value) -> Vec<String> {
    value_array(record, "metrics")
        .iter()
        .take(8)
        .filter_map(|metric| {
            Some(format!(
                "{}={}",
                metric.get("name")?.as_str()?,
                metric.get("value")?
            ))
        })
        .collect()
}

fn case_signal_lines(record: &Value) -> Vec<String> {
    value_array(record, "cases")
        .iter()
        .filter(|case| !case.get("passed").and_then(Value::as_bool).unwrap_or(false))
        .take(8)
        .map(|case| {
            format!(
                "{} failed: {}",
                string_field(case, "case_id"),
                string_field(case, "message")
            )
        })
        .collect()
}

fn patch_changed_paths(path: &PathBuf, run: Option<&Value>) -> Vec<String> {
    if let Some(run) = run {
        let paths = changed_paths(run);
        if !paths.is_empty() {
            return paths;
        }
    }
    fs::read_to_string(path)
        .map(|diff| changed_paths_from_diff(&diff))
        .unwrap_or_default()
}

fn value_array<'a>(record: &'a Value, name: &str) -> &'a [Value] {
    record
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn string_array(record: &Value, name: &str) -> Vec<String> {
    value_array(record, name)
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn field_string(record: &Value, name: &str) -> String {
    record.get(name).map(Value::to_string).unwrap_or_default()
}

fn string_field(record: &Value, name: &str) -> String {
    record
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn safe_id(value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(160)
        .collect::<String>();
    if slug.is_empty() {
        "memory".to_owned()
    } else {
        slug
    }
}

fn safe_tag(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '/' | '-') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde_json::{Value, json};

    use super::*;

    #[test]
    fn rejected_summary_includes_changes_and_score_delta() {
        let record = rejected_record("current", "2");

        let summary = primary_summary("rejected_attempt", &record);

        assert!(summary.contains("Score delta: -0.010000"));
        assert!(summary.contains("Changed paths: src/query.rs"));
        assert!(summary.contains("Top improvements: metric:relay_teams_query_p95_ms"));
        assert!(summary.contains("Top degradations: score_component:score"));
    }

    #[test]
    fn accepted_summary_lists_protected_floors() {
        let record = json!({
            "run_id": "accepted",
            "accepted": true,
            "score": 0.8,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "semantic_vector": 0.0,
            "stability": 1.0,
            "improvements": [{"kind": "score_component", "name": "score", "previous": 0.7, "current": 0.8}],
            "degradations": [],
            "optimization_plan": {"changed_paths": ["src/query.rs"]},
        });

        let summary = primary_summary("accepted_optimization", &record);

        assert!(summary.contains("Protected floors: foundational=1.0"));
        assert!(summary.contains("Key improvements: score_component:score"));
        assert!(summary.contains("Known degradations: none recorded"));
    }

    #[test]
    fn repeated_rejection_cluster_memory_is_recorded() {
        let workspace = temp_workspace("memory-cluster");
        let paths = history::HistoryPaths::new(&workspace);
        paths.ensure().expect("history paths");
        let previous = rejected_record("previous", "1");
        history::append_run(&paths, &previous).expect("previous run");
        let current = rejected_record("current", "2");

        write_run_memory(&paths, &current).expect("memory");
        let index = fs::read_to_string(&paths.memory_index).expect("index");

        assert!(index.contains("repeated_rejection_cluster"));
        assert!(index.contains("current-repeated_rejection_cluster"));
    }

    fn rejected_record(run_id: &str, timestamp: &str) -> Value {
        json!({
            "run_id": run_id,
            "timestamp": timestamp,
            "accepted": false,
            "score": 0.79,
            "foundational_capability": 1.0,
            "competitive_capability": 0.8,
            "semantic_vector": 0.0,
            "stability": 1.0,
            "reject_reasons": ["candidate did not improve score or tracked objectives beyond epsilon"],
            "improvements": [{"kind": "metric", "name": "relay_teams_query_p95_ms", "previous": 8000.0, "current": 7000.0}],
            "degradations": [{"kind": "score_component", "name": "score", "previous": 0.8, "current": 0.79}],
            "optimization_plan": {"changed_paths": ["src/query.rs"]},
            "patch": {"path": "/tmp/current.patch"},
            "report": "/tmp/current.json",
            "gates": [],
        })
    }

    fn temp_workspace(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let workspace = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        fs::create_dir_all(workspace.join(".git")).expect("workspace");
        workspace
    }
}
