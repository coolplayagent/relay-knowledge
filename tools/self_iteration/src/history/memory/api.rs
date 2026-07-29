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
