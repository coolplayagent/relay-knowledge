//! Static configuration registry extraction, independent of callable graph resolution.
use super::{FeatureFlagFileInput, feature_flag_record_from_range};
use crate::code::config_files::ConfigRange;
use crate::domain::{CodeConfigMetadata, CodeFeatureFlagRecord, DomainError};
mod files;
mod java;
mod shell;

pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    match input.language_id {
        "java" => java::extract(input),
        "properties" | "ini" | "gotemplate" | "bash" => files::extract(input),
        _ => Ok(Vec::new()),
    }
}

fn record(
    input: &FeatureFlagFileInput<'_>,
    kind: &str,
    key: &str,
    edge: &str,
    start: usize,
    end: usize,
) -> Result<CodeFeatureFlagRecord, DomainError> {
    let range = ConfigRange {
        byte_start: start,
        byte_end: end,
        line_start: input.content[..start]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
            + 1,
        line_end: input.content[..end].bytes().filter(|b| *b == b'\n').count() + 1,
    };
    let mut result = feature_flag_record_from_range(
        input,
        kind,
        key,
        edge,
        range,
        input.content[start..end].trim(),
    )?;
    result.metadata = metadata(input, start);
    Ok(result)
}

fn metadata(input: &FeatureFlagFileInput<'_>, start: usize) -> CodeConfigMetadata {
    let format = match input.language_id {
        "gotemplate" => "ctmpl",
        "bash" => "shell",
        other => other,
    };
    let mut meta = CodeConfigMetadata {
        source_format: format.to_owned(),
        ..Default::default()
    };
    // Only an adjacent explicit annotation supplies domain/hot-reload evidence.
    for line in input.content[..start].lines().rev().take(3) {
        let line = line.trim();
        if let Some((_, annotation)) = line.split_once("@config ") {
            for part in annotation.split_whitespace() {
                if let Some((key, value)) = part.split_once('=') {
                    match key {
                        "domain" => meta.domain = Some(value.to_ascii_lowercase()),
                        "hot-reload" => meta.hot_reload = value.parse().ok(),
                        _ => {}
                    }
                }
            }
            break;
        }
        if !line.is_empty() && !line.starts_with(['#', '!', '/', '*']) {
            break;
        }
    }
    meta
}
fn value_type(value: &str) -> &'static str {
    if matches!(value, "true" | "false") {
        "boolean"
    } else if value.parse::<i64>().is_ok() {
        "integer"
    } else if value.parse::<f64>().is_ok() {
        "number"
    } else {
        "string"
    }
}
#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
