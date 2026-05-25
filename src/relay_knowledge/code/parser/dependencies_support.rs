use serde_json::Value;

pub(super) fn cargo_lock_source_is_external(source: Option<&str>) -> bool {
    source.is_some_and(|source| {
        source.starts_with("registry+")
            || source.starts_with("git+")
            || source.starts_with("sparse+")
    })
}

pub(super) fn npm_requirement_is_local(requirement: &str) -> bool {
    let requirement = requirement.trim();
    requirement.starts_with('.')
        || requirement.starts_with('/')
        || requirement.starts_with("~/")
        || ["file:", "link:", "portal:", "workspace:"]
            .iter()
            .any(|prefix| requirement.starts_with(prefix))
}

pub(super) fn package_lock_entry_is_local(package: &Value) -> bool {
    package.get("link").and_then(Value::as_bool) == Some(true)
        || package
            .get("resolved")
            .and_then(Value::as_str)
            .is_some_and(npm_requirement_is_local)
}

pub(super) fn package_lock_package_name(path: &str, package: &Value) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    package
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| package_lock_package_name_from_path(path))
}

fn package_lock_package_name_from_path(path: &str) -> Option<String> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let mut package_name = None;
    while let Some(segment) = segments.next() {
        if segment != "node_modules" {
            continue;
        }
        let Some(first) = segments.next() else {
            continue;
        };
        package_name = if first.starts_with('@') {
            segments.next().map(|name| format!("{first}/{name}"))
        } else {
            Some(first.to_owned())
        };
    }
    package_name
}

pub(super) fn requirements_dependency_line(line: &str) -> Option<&str> {
    let trimmed = strip_requirement_comment(line).trim();
    if trimmed.is_empty() {
        return None;
    }
    for prefix in ["-e ", "-e\t", "--editable ", "--editable\t"] {
        if let Some(requirement) = trimmed.strip_prefix(prefix) {
            return Some(requirement.trim()).filter(|requirement| !requirement.is_empty());
        }
    }
    (!trimmed.starts_with('-')).then_some(trimmed)
}

fn strip_requirement_comment(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return "";
    }
    for (index, character) in line.char_indices() {
        if character == '#'
            && line[..index]
                .chars()
                .last()
                .is_some_and(char::is_whitespace)
        {
            return &line[..index];
        }
    }
    line
}

pub(super) fn python_requirement(value: &str) -> Option<(String, Option<String>)> {
    let value = value.trim().trim_matches(',').trim();
    if value.is_empty() {
        return None;
    }
    let value = value.split_once(';').map_or(value, |(left, _)| left).trim();
    let (version_input, direct_reference) = split_direct_reference(value);
    if direct_reference.is_some_and(python_reference_is_local) {
        return None;
    }
    if direct_reference.is_none() {
        if let Some(name) = requirement_egg_name(value) {
            return Some((name, Some(format!("@ {value}"))));
        }
        if python_reference_is_local(value) {
            return None;
        }
    }
    let split_at = version_input
        .find(['=', '<', '>', '~', '!'])
        .unwrap_or(version_input.len());
    let name = version_input[..split_at]
        .split_once('[')
        .map(|(left, _)| left)
        .unwrap_or(&version_input[..split_at])
        .trim();
    if name.is_empty() {
        return None;
    }
    if python_name_is_local_path(name) {
        return None;
    }
    let requirement = direct_reference
        .map(|reference| format!("@ {}", reference.trim()))
        .or_else(|| version_requirement(version_input, split_at));
    Some((name.to_owned(), requirement))
}

fn split_direct_reference(value: &str) -> (&str, Option<&str>) {
    let Some(index) = value.find(" @ ") else {
        return (value, None);
    };
    let name = value[..index].trim();
    let reference = value[index + 3..].trim();
    if name.is_empty() || reference.is_empty() {
        (value, None)
    } else {
        (name, Some(reference))
    }
}

fn requirement_egg_name(value: &str) -> Option<String> {
    let fragment = value.split_once('#')?.1;
    fragment
        .split('&')
        .find_map(|part| part.strip_prefix("egg="))
        .map(|name| {
            name.split_once('[')
                .map_or(name, |(left, _)| left)
                .trim()
                .to_owned()
        })
        .filter(|name| !name.is_empty())
}

fn python_reference_is_local(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('.')
        || value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("file:")
}

fn python_name_is_local_path(name: &str) -> bool {
    name.contains('/') || name.contains('\\') || python_reference_is_local(name)
}

fn version_requirement(value: &str, split_at: usize) -> Option<String> {
    value
        .get(split_at..)
        .map(str::trim)
        .filter(|requirement| !requirement.is_empty())
        .map(str::to_owned)
}

pub(super) fn inline_table_field(value: &str, field: &str) -> Option<String> {
    let after_equals = inline_table_value(value, field)?;
    let quote = after_equals.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = after_equals.get(1..)?.find(quote)?;
    Some(after_equals[1..1 + end].to_owned())
}

pub(super) fn inline_table_bool_field(value: &str, field: &str) -> Option<bool> {
    let after_equals = inline_table_value(value, field)?;
    if after_equals.starts_with("true") {
        Some(true)
    } else if after_equals.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

fn inline_table_value<'a>(value: &'a str, field: &str) -> Option<&'a str> {
    let mut start = 0;
    let body = inline_table_body(value);
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in body.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' && active_quote == '"' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character == ',' {
            if let Some(result) = inline_table_entry_value(&body[start..index], field) {
                return Some(result);
            }
            start = index + character.len_utf8();
        }
    }
    inline_table_entry_value(&body[start..], field)
}

fn inline_table_body(value: &str) -> &str {
    let value = value.trim();
    value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .unwrap_or(value)
        .trim()
}

fn inline_table_entry_value<'a>(entry: &'a str, field: &str) -> Option<&'a str> {
    let (key, raw_value) = entry.split_once('=')?;
    if key.trim() == field {
        Some(raw_value.trim())
    } else {
        None
    }
}

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
