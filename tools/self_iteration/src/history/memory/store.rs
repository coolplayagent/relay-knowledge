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
