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
    if latest
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
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
                if run
                    .get("accepted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "accepted"
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
    items.sort_by(|left, right| {
        string_field(right, "created_at").cmp(&string_field(left, "created_at"))
    });
    items
}

fn primary_kind(record: &Value) -> String {
    if record
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
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
            "Accepted run {} scored {}. Changed paths: {}. Key improvements: {}.",
            run_id,
            record
                .get("score")
                .map(Value::to_string)
                .unwrap_or_default(),
            changed_paths(record).join(", "),
            compact_score_changes(value_array(record, "improvements"), 4).join("; ")
        )
    } else if kind == "quality_gate_failure" {
        format!(
            "Rejected run {} failed quality gates {}. Inspect detail before retrying related changes.",
            run_id,
            failed_gate_names(record).join(", ")
        )
    } else {
        format!(
            "Rejected run {} scored {}. Reasons: {}.",
            run_id,
            record
                .get("score")
                .map(Value::to_string)
                .unwrap_or_default(),
            string_array(record, "reject_reasons").join("; ")
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
        "accepted": record.get("accepted").and_then(Value::as_bool).unwrap_or(false),
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
        if record
            .get("accepted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
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
