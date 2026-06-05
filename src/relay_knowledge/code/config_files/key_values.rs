use super::{
    languages::{json, properties, yaml},
    model::ConfigFact,
    source::{
        config_key_prefix, push_boolean_definition, push_definition, source_lines, valid_config_key,
    },
};

pub(super) fn facts(language_id: &str, content: &str, definitions: &mut Vec<ConfigFact>) {
    let mut yaml_block = yaml::BlockScalarTracker::default();
    let mut properties_continuation = false;
    for line in source_lines(content) {
        let trimmed = line.text.trim();
        if language_id == "yaml" && yaml_block.should_skip(line.text, trimmed) {
            continue;
        }
        if language_id == "properties"
            && properties::skip_continued_value_line(
                line.text,
                trimmed,
                &mut properties_continuation,
            )
        {
            continue;
        }
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with('!')
            || trimmed.starts_with("//")
        {
            continue;
        }
        for key in boolean_config_keys(language_id, trimmed) {
            push_boolean_definition(definitions, key, "config_key", line.range());
        }
        if language_id == "json" {
            for key in json::object_keys(trimmed)
                .into_iter()
                .filter(|key| valid_config_key(key))
            {
                push_definition(definitions, key, "config_key", line.range());
            }
            continue;
        }
        if language_id == "toml"
            && let Some(section) = toml_section_name(trimmed)
        {
            push_definition(definitions, section, "section", line.range());
            continue;
        }
        if language_id == "ini" && trimmed.starts_with('[') && trimmed.ends_with(']') {
            push_definition(
                definitions,
                &trimmed[1..trimmed.len() - 1],
                "section",
                line.range(),
            );
            continue;
        }
        let key = if language_id == "yaml" {
            yaml::mapping_key(trimmed).map(config_key_prefix)
        } else {
            trimmed
                .split_once('=')
                .or_else(|| trimmed.split_once(':'))
                .map(|(key, _)| config_key_prefix(key))
                .or_else(|| properties_space_key(language_id, trimmed))
        };
        if let Some(key) = key.filter(|key| valid_config_key(key)) {
            push_definition(definitions, key, "config_key", line.range());
        }
    }
}

fn boolean_config_keys(language_id: &str, line: &str) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(key) = direct_boolean_config_key(line) {
        push_unique(&mut keys, key);
    }
    if let Some(key) = properties_boolean_config_key(language_id, line) {
        push_unique(&mut keys, key);
    }
    for key in inline_boolean_config_keys(line) {
        push_unique(&mut keys, key);
    }

    keys
}

fn direct_boolean_config_key(line: &str) -> Option<String> {
    let separator = line
        .find('=')
        .or_else(|| line.find(':'))
        .filter(|index| *index > 0)?;
    let (key, value) = line.split_at(separator);
    boolean_config_key(key, &value[1..])
}

fn properties_boolean_config_key(language_id: &str, line: &str) -> Option<String> {
    if language_id != "properties" {
        return None;
    }
    let (key, value) = line.split_once(char::is_whitespace)?;
    boolean_config_key(key, value)
}

fn boolean_config_key(key: &str, value: &str) -> Option<String> {
    let key = config_key_prefix(key);
    if !valid_config_key(key) {
        return None;
    }
    let value = value.trim().trim_end_matches(',');
    if value.starts_with('"') || value.starts_with('\'') {
        return None;
    }
    boolean_literal(value).then(|| key.to_owned())
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

fn boolean_literal(value: &str) -> bool {
    matches!(value, "true" | "false" | "enabled" | "disabled")
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

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn toml_section_name(value: &str) -> Option<&str> {
    value
        .strip_prefix("[[")
        .and_then(|section| section.strip_suffix("]]"))
        .or_else(|| {
            value
                .strip_prefix('[')
                .and_then(|section| section.strip_suffix(']'))
        })
        .map(str::trim)
        .filter(|section| valid_config_key(section))
}

fn properties_space_key<'a>(language_id: &str, value: &'a str) -> Option<&'a str> {
    (language_id == "properties")
        .then(|| value.split_whitespace().next())
        .flatten()
}
