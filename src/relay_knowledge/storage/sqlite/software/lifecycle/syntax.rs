//! Small syntax recognizers shared by lifecycle projection extractors.

use std::path::Path;

use super::document::IndexedLine;

pub(super) fn file_name(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

pub(super) fn file_stem(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

pub(super) fn key_value(line: &str, separator: char) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(separator)?;
    Some((key.trim(), value.trim()))
}

pub(super) fn toml_section(line: &str) -> Option<&str> {
    line.strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
        .or_else(|| {
            line.strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
        })
        .map(str::trim)
}

pub(super) fn toml_value(line: &str, key: &str) -> Option<String> {
    let (candidate, value) = key_value(line, '=')?;
    (candidate == key).then(|| clean_scalar(value))
}

pub(super) fn yaml_value(line: &str, key: &str) -> Option<String> {
    let (candidate, value) = key_value(line, ':')?;
    (candidate == key && !value.is_empty()).then(|| clean_scalar(value))
}

pub(super) fn json_string_value(line: &str, key: &str) -> Option<String> {
    let (candidate, value) = json_string_pair(line)?;
    (candidate == key).then_some(value)
}

pub(super) fn json_string_pair(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim().trim_end_matches(',');
    let trimmed = trimmed.strip_prefix('"')?;
    let (key, rest) = trimmed.split_once('"')?;
    let value = rest.trim_start().strip_prefix(':')?.trim();
    Some((key.to_owned(), clean_scalar(value)))
}

pub(super) fn clean_scalar(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(',')
        .trim_end_matches(')')
        .trim_start_matches('(')
        .trim_matches('"')
        .trim_matches('\'')
        .to_owned()
}

pub(super) fn strip_comment(line: &str, marker: char) -> &str {
    line.split_once(marker).map_or(line, |(value, _)| value)
}

pub(super) fn first_call_arg(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?.trim();
    let rest = rest.trim_start_matches('(').trim();
    let token = rest
        .split([',', ')', ' ', '\t'])
        .find(|value| !value.trim().is_empty())?;
    Some(clean_scalar(token))
}

pub(super) fn gradle_plugin(line: &str) -> Option<String> {
    line.strip_prefix("id ")
        .or_else(|| line.strip_prefix("id("))
        .map(clean_scalar)
}

pub(super) fn terraform_block(line: &str, prefix: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix(prefix)?.trim();
    let mut quoted = rest.split('"').skip(1).step_by(2);
    let first = quoted.next()?.to_owned();
    let second = quoted.next().unwrap_or(&first).to_owned();
    Some((first, second))
}

pub(super) fn xml_string(line: &str) -> Option<String> {
    line.split_once("<string>")
        .and_then(|(_, rest)| rest.split_once("</string>"))
        .map(|(value, _)| value.trim().to_owned())
}

pub(super) fn indentation(line: &str) -> usize {
    line.chars().take_while(|value| *value == ' ').count()
}

pub(super) fn markdown_heading(line: &str) -> Option<String> {
    let hashes = line.chars().take_while(|value| *value == '#').count();
    if !(1..=4).contains(&hashes) {
        return None;
    }
    let title = line[hashes..].trim();
    (!title.is_empty()).then(|| title.to_owned())
}

pub(super) fn design_heading_kind(title: &str, _path: &str) -> Option<&'static str> {
    let lower = title.to_ascii_lowercase();
    if lower.contains("architecture") || lower.contains("design") {
        Some("architecture")
    } else if lower.contains("module") {
        Some("module")
    } else if lower.contains("component") {
        Some("component")
    } else if lower.contains("interface") || lower.contains("api") {
        Some("interface")
    } else if lower.contains("capability") || lower.contains("feature") {
        Some("capability")
    } else {
        None
    }
}

pub(super) fn next_markdown_summary(lines: &[IndexedLine]) -> Option<String> {
    lines
        .iter()
        .map(|line| line.text.trim())
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.chars().take(240).collect())
}

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
