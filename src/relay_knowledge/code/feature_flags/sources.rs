const CONFIG_RECEIVERS: &[&str] = &[
    "config",
    "settings",
    "feature_flags",
    "flags",
    "toggles",
    "options",
];
const CONFIG_METHODS: &[&str] = &[
    ".get(",
    ".get_bool(",
    ".getBoolean(",
    ".get_boolean(",
    ".enabled(",
    ".is_enabled(",
];

pub(super) fn env_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for pattern in [
        "std::env::var(",
        "std::env::var_os(",
        "env::var(",
        "std::getenv(",
        "getenv(",
        "os.getenv(",
        "System.getenv(",
    ] {
        collect_quoted_arguments(line, pattern, &mut keys);
    }
    collect_dotted_members(line, "process.env.", &mut keys);
    collect_bracket_keys(line, "process.env[", &mut keys);

    keys
}

pub(super) fn config_read_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for receiver in CONFIG_RECEIVERS {
        for method in CONFIG_METHODS {
            collect_quoted_arguments(line, &format!("{receiver}{method}"), &mut keys);
        }
    }

    keys
}

pub(super) fn preprocessor_flag_keys(line: &str, language_id: &str) -> Vec<String> {
    if !matches!(language_id, "c" | "cpp" | "csharp") {
        return Vec::new();
    }
    let trimmed = line.trim_start();
    let remainder = if let Some(remainder) = trimmed.strip_prefix("#ifdef") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#ifndef") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#elif") {
        remainder
    } else if let Some(remainder) = trimmed.strip_prefix("#if") {
        remainder
    } else {
        return Vec::new();
    };

    let mut keys = Vec::new();
    collect_preprocessor_identifiers(remainder, &mut keys);

    keys
}

fn collect_preprocessor_identifiers(value: &str, keys: &mut Vec<String>) {
    let mut current = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            current.push(character);
            continue;
        }
        push_preprocessor_identifier(keys, &mut current);
    }
    push_preprocessor_identifier(keys, &mut current);
}

fn push_preprocessor_identifier(keys: &mut Vec<String>, current: &mut String) {
    if valid_preprocessor_key(current) {
        push_unique(keys, current.clone());
    }
    current.clear();
}

fn valid_preprocessor_key(key: &str) -> bool {
    valid_source_key(key)
        && key
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && !matches!(
            key,
            "defined" | "if" | "ifdef" | "ifndef" | "elif" | "true" | "false"
        )
}

fn collect_quoted_arguments(line: &str, pattern: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(pattern_start) = find_code_pattern(line, pattern, start) {
        let value_start = pattern_start + pattern.len();
        if let Some(key) = quoted_prefix(&line[value_start..]) {
            push_unique(keys, key);
        }
        start = value_start.saturating_add(1);
    }
}

fn collect_bracket_keys(line: &str, pattern: &str, keys: &mut Vec<String>) {
    collect_quoted_arguments(line, pattern, keys);
}

fn collect_dotted_members(line: &str, pattern: &str, keys: &mut Vec<String>) {
    let mut start = 0usize;
    while let Some(pattern_start) = find_code_pattern(line, pattern, start) {
        let member_start = pattern_start + pattern.len();
        let member = line[member_start..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if valid_source_key(&member) {
            push_unique(keys, member.clone());
        }
        start = member_start.saturating_add(member.len().max(1));
    }
}

fn find_code_pattern(line: &str, pattern: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0usize;
    while index < line.len() {
        let rest = &line[index..];
        let character = rest.chars().next()?;
        if let Some(quote_character) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote_character {
                quote = None;
            }
            index = index.saturating_add(character.len_utf8());
            continue;
        }

        if index >= start && rest.starts_with(pattern) {
            return Some(index);
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
        }
        index = index.saturating_add(character.len_utf8());
    }

    None
}

fn quoted_prefix(value: &str) -> Option<String> {
    let value = value.trim_start();
    let mut chars = value.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = value[1..].find(quote)?;
    let key = &value[1..1 + end];
    valid_source_key(key).then(|| key.to_owned())
}

fn valid_source_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 160
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
}

pub(super) fn usage_edge_kind(line: &str) -> &'static str {
    if line_looks_conditional(line) {
        "guards_code"
    } else {
        "reads_config"
    }
}

fn line_looks_conditional(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("if ")
        || trimmed.starts_with("if(")
        || trimmed.starts_with("elif ")
        || trimmed.starts_with("else if")
        || trimmed.starts_with("while ")
        || trimmed.contains(" if ")
        || trimmed.contains(" ? ")
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}
