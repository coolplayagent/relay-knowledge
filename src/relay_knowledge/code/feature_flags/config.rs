const CONFIG_EXTENSIONS: &[&str] = &[
    ".toml",
    ".yaml",
    ".yml",
    ".json",
    ".env",
    ".ini",
    ".properties",
];

pub(super) fn boolean_config_key(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let separator = trimmed
        .find('=')
        .or_else(|| trimmed.find(':'))
        .filter(|index| *index > 0)?;
    let (key, value) = trimmed.split_at(separator);
    let key = key.trim().trim_matches('"').trim_matches('\'');
    let value = value[1..]
        .trim()
        .trim_end_matches(',')
        .trim_matches('"')
        .trim_matches('\'');
    if !matches!(value, "true" | "false" | "enabled" | "disabled") || key.is_empty() {
        return None;
    }
    if key
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        Some(key.to_owned())
    } else {
        None
    }
}

pub(super) fn looks_like_config_file(path: &str) -> bool {
    CONFIG_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
}
