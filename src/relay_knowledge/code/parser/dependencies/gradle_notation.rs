//! Parses Gradle dependency calls and Maven coordinate notation.

pub(super) fn gradle_dependency_call(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let config_end =
        trimmed.find(|character: char| character.is_whitespace() || character == '(')?;
    let configuration = trimmed[..config_end].trim();
    if configuration.is_empty() || matches!(configuration, "id" | "alias") {
        return None;
    }
    gradle_map_coordinate(&trimmed[config_end..])
        .or_else(|| first_quoted_value(trimmed).map(str::to_owned))
        .map(|dependency| (configuration.to_owned(), dependency))
}

fn gradle_map_coordinate(value: &str) -> Option<String> {
    let group = gradle_named_argument(value, "group")?;
    let name = gradle_named_argument(value, "name")?;
    if group.is_empty() || name.is_empty() {
        return None;
    }
    match gradle_named_argument(value, "version") {
        Some(version) if !version.is_empty() => Some(format!("{group}:{name}:{version}")),
        _ => Some(format!("{group}:{name}")),
    }
}

fn gradle_named_argument(value: &str, field: &str) -> Option<String> {
    let mut offset = 0;
    while let Some(relative_start) = value[offset..].find(field) {
        let start = offset + relative_start;
        let end = start + field.len();
        offset = end;
        if start > 0 && is_identifier_byte(value.as_bytes()[start - 1]) {
            continue;
        }
        let rest = value[end..].trim_start();
        let Some(delimiter) = rest.chars().next() else {
            continue;
        };
        if delimiter != ':' && delimiter != '=' {
            continue;
        }
        return gradle_argument_value(rest[delimiter.len_utf8()..].trim_start());
    }
    None
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn gradle_argument_value(value: &str) -> Option<String> {
    let first = value.chars().next()?;
    if matches!(first, '"' | '\'') {
        return value[first.len_utf8()..]
            .split_once(first)
            .map(|(left, _)| left.to_owned());
    }
    let end = value
        .find(|character: char| character == ',' || character == ')' || character.is_whitespace())
        .unwrap_or(value.len());
    Some(value[..end].trim().to_owned()).filter(|value| !value.is_empty())
}

fn first_quoted_value(value: &str) -> Option<&str> {
    let mut start = None::<usize>;
    let mut quote = '\0';
    for (index, character) in value.char_indices() {
        if start.is_none() && matches!(character, '"' | '\'') {
            start = Some(index + character.len_utf8());
            quote = character;
        } else if start.is_some() && character == quote {
            let value_start = start.unwrap_or_default();
            return Some(&value[value_start..index]);
        }
    }
    None
}

pub(super) fn gradle_coordinate_parts(
    value: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    if value.contains(':') {
        let mut parts = value.split(':');
        return (
            parts
                .next()
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
            parts
                .next()
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
            parts
                .next()
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
        );
    }
    (None, None, None)
}
