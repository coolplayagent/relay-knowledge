//! Scalar configuration declarations, template keys and shell environment defaults.

use crate::domain::{CodeFeatureFlagMetadata, CodeFeatureFlagRecord, DomainError};

use crate::code::config_files::ConfigRange;
use crate::code::feature_flags::{FeatureFlagFileInput, feature_flag_record_from_range};
pub(in crate::code) mod shell;
mod shell_bindings;
mod template_commands;
mod template_literals;
mod template_reads;

pub(in crate::code::feature_flags) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let format = match input.language_id {
        "properties" | "ini" => input.language_id,
        _ if input
            .path
            .rsplit('.')
            .next()
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case("ctmpl")) =>
        {
            "ctmpl"
        }
        _ => "",
    };
    if !matches!(format, "properties" | "ini" | "ctmpl") {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    let outside = if format == "ctmpl" {
        Some(template_reads::collect(input, &mut records)?)
    } else {
        None
    };
    let declaration_content = outside.as_deref().unwrap_or(input.content);
    let mut facts_by_line = std::collections::BTreeMap::<usize, Vec<_>>::new();
    for fact in input
        .config_facts
        .iter()
        .filter(|fact| fact.kind == "config_key")
    {
        facts_by_line
            .entry(fact.range.line_start)
            .or_default()
            .push(fact);
    }
    let mut offset = 0;
    let mut declaration = CodeFeatureFlagMetadata::default();
    for (index, segment) in declaration_content.split_inclusive('\n').enumerate() {
        let line = segment.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();
        let range = ConfigRange {
            byte_start: offset,
            byte_end: offset + line.len(),
            line_start: index + 1,
            line_end: index + 1,
        };
        let original = &input.content[range.byte_start..range.byte_end];
        offset += segment.len();
        if let Some(annotation) = trimmed
            .strip_prefix("# @config ")
            .or_else(|| trimmed.strip_prefix("; @config "))
        {
            declaration = annotation_metadata(annotation);
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with(['#', ';', '!']) {
            declaration = CodeFeatureFlagMetadata::default();
            continue;
        }
        let definitions = if format == "ctmpl" {
            assignment(trimmed)
                .map(|(key, value)| (key, value, range))
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            facts_by_line
                .get(&(index + 1))
                .into_iter()
                .flatten()
                .map(|fact| {
                    (
                        fact.name.as_str(),
                        scalar_value(trimmed, &fact.name).unwrap_or_default(),
                        fact.range,
                    )
                })
                .collect()
        };
        for (key, value, range) in definitions {
            let mut record = feature_flag_record_from_range(
                input,
                "config_key",
                key,
                "defines_config",
                range,
                original.trim(),
            )?;
            record.metadata = declaration.clone();
            record.metadata.source_format = format.to_owned();
            if !value.is_empty()
                && original == line
                && !value.contains("{{")
                && !value.contains('$')
                && !value.contains('\\')
            {
                record.metadata.default_value = Some(value.to_owned());
                record.metadata.value_type = Some(value_type(value).to_owned());
            }
            records.push(record);
        }
        declaration = CodeFeatureFlagMetadata::default();
    }
    Ok(records)
}

fn assignment(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if !valid_key(key) {
        return None;
    }
    Some((key, value.trim().trim_matches(['\'', '"'])))
}

fn scalar_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let remainder = line.strip_prefix(key)?;
    let remainder = remainder.trim_start();
    let value = remainder
        .strip_prefix(['=', ':'])
        .unwrap_or(remainder)
        .trim();
    Some(value)
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 512
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn value_type(value: &str) -> &'static str {
    if matches!(value, "true" | "false" | "enabled" | "disabled") {
        "boolean"
    } else if value.parse::<i64>().is_ok() {
        "integer"
    } else if value.parse::<f64>().is_ok() {
        "number"
    } else {
        "string"
    }
}

fn annotation_metadata(annotation: &str) -> CodeFeatureFlagMetadata {
    let mut metadata = CodeFeatureFlagMetadata::default();
    for token in annotation.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        match (key, value) {
            ("domain", value) if valid_key(value) => metadata.domain = Some(value.to_owned()),
            ("hot-reload", "true") => metadata.hot_reload = Some(true),
            ("hot-reload", "false") => metadata.hot_reload = Some(false),
            _ => {}
        }
    }
    metadata
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
