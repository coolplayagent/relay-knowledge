//! Parses Python requirement lines, direct references, and version constraints.

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
    for (index, _) in value.match_indices('@') {
        let name = value[..index].trim();
        let reference = value[index + 1..].trim();
        if python_direct_reference_name(name) && !reference.is_empty() {
            return (name, Some(reference));
        }
    }

    (value, None)
}

fn python_direct_reference_name(value: &str) -> bool {
    let name = value.split_once('[').map_or(value, |(name, _)| name).trim();
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        && name
            .chars()
            .any(|character| character.is_ascii_alphanumeric())
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
