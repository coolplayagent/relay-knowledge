//! Scalar configuration declarations, template keys and shell environment defaults.

use crate::domain::{CodeFeatureFlagMetadata, CodeFeatureFlagRecord, DomainError};

use crate::code::config_files::ConfigRange;
use crate::code::feature_flags::{FeatureFlagFileInput, feature_flag_record_from_range};
pub(in crate::code) mod shell;
mod shell_bindings;

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
    for (index, segment) in input.content.split_inclusive('\n').enumerate() {
        let line = segment.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim();
        let range = ConfigRange {
            byte_start: offset,
            byte_end: offset + line.len(),
            line_start: index + 1,
            line_end: index + 1,
        };
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
                trimmed,
            )?;
            record.metadata = declaration.clone();
            record.metadata.source_format = format.to_owned();
            if !value.is_empty()
                && !value.contains("{{")
                && !value.contains('$')
                && !value.contains('\\')
            {
                record.metadata.default_value = Some(value.to_owned());
                record.metadata.value_type = Some(value_type(value).to_owned());
            }
            records.push(record);
        }
        if format == "ctmpl" {
            collect_template_reads(input, trimmed, range, &mut records)?;
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

fn collect_template_reads(
    input: &FeatureFlagFileInput<'_>,
    line: &str,
    range: ConfigRange,
    records: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    let mut remaining = line;
    while let Some((_, after)) = remaining.split_once("{{") {
        let Some((action, rest)) = after.split_once("}}") else {
            break;
        };
        let action = action.trim().trim_start_matches('-').trim();
        let (function, arguments) = action
            .split_once(char::is_whitespace)
            .unwrap_or((action, ""));
        let kind = match function {
            "key" | "keyOrDefault" => Some("config_key"),
            "env" => Some("env_var"),
            _ => None,
        };
        if let (Some(kind), Some((key, arguments))) = (kind, template_string(arguments)) {
            if valid_key(&key) {
                let mut record =
                    feature_flag_record_from_range(input, kind, &key, "reads_config", range, line)?;
                record.metadata.source_format = "ctmpl".to_owned();
                if function == "keyOrDefault" {
                    if let Some((value, _)) = template_string(arguments) {
                        record.metadata.value_type = Some(value_type(&value).to_owned());
                        record.metadata.default_value = Some(value);
                    }
                }
                records.push(record);
            }
        }
        remaining = rest;
    }
    Ok(())
}

fn template_string(arguments: &str) -> Option<(String, &str)> {
    let arguments = arguments.trim_start();
    if let Some(raw) = arguments.strip_prefix('`') {
        let (value, rest) = raw.split_once('`')?;
        return Some((value.to_owned(), rest));
    }
    let mut stream = serde_json::Deserializer::from_str(arguments).into_iter::<String>();
    let value = stream.next()?.ok()?;
    Some((value, &arguments[stream.byte_offset()..]))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
