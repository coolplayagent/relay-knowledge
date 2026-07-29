pub fn load_cases(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut config = serde_json::from_str::<Value>(&text)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    let include_files = config
        .as_object_mut()
        .and_then(|object| object.remove("include_files"))
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    for include_file in include_files {
        let relative = include_file
            .as_str()
            .ok_or_else(|| format!("invalid include file entry in {}", path.display()))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        let included = load_cases(&parent.join(relative))?;
        merge_case_config(&mut config, included)?;
    }
    Ok(config)
}
