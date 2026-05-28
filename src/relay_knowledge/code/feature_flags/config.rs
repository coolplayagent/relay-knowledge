const CONFIG_EXTENSIONS: &[&str] = &[
    ".toml",
    ".yaml",
    ".yml",
    ".json",
    ".env",
    ".ini",
    ".properties",
];

pub(super) fn boolean_config_keys(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let mut keys = Vec::new();
    if let Some(key) = direct_boolean_config_key(trimmed) {
        push_unique(&mut keys, key);
    }
    for key in inline_boolean_config_keys(trimmed) {
        push_unique(&mut keys, key);
    }

    keys
}

fn direct_boolean_config_key(trimmed: &str) -> Option<String> {
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

fn inline_boolean_config_keys(line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for value_start in boolean_value_starts(line) {
        let prefix = line[..value_start].trim_end();
        let Some(separator) = prefix.rfind(['=', ':']) else {
            continue;
        };
        if !prefix[separator.saturating_add(1)..].trim().is_empty() {
            continue;
        }
        let Some(key) = prefix[..separator]
            .trim_end()
            .rsplit(|character: char| {
                character.is_whitespace() || matches!(character, '{' | '[' | ',')
            })
            .next()
        else {
            continue;
        };
        let key = key.trim().trim_matches('"').trim_matches('\'');
        if valid_config_key(key) {
            push_unique(&mut keys, key.to_owned());
        }
    }

    keys
}

fn boolean_value_starts(line: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0usize;
    while index < line.len() {
        let rest = &line[index..];
        let Some(character) = rest.chars().next() else {
            break;
        };
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
        if character == '"' || character == '\'' {
            quote = Some(character);
            index = index.saturating_add(character.len_utf8());
            continue;
        }
        for marker in ["true", "false", "enabled", "disabled"] {
            if rest.starts_with(marker)
                && token_boundary_before(line, index)
                && token_boundary_after(line, index + marker.len())
            {
                starts.push(index);
            }
        }
        index = index.saturating_add(character.len_utf8());
    }
    starts.sort_unstable();
    starts
}

fn token_boundary_before(line: &str, start: usize) -> bool {
    line[..start]
        .chars()
        .next_back()
        .is_none_or(|character| !is_config_token_character(character))
}

fn token_boundary_after(line: &str, end: usize) -> bool {
    line[end..]
        .chars()
        .next()
        .is_none_or(|character| !is_config_token_character(character))
}

fn is_config_token_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}

fn valid_config_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

pub(super) fn looks_like_config_file(path: &str) -> bool {
    CONFIG_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
}
