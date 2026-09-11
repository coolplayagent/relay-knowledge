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

fn check_fact_budget(count: usize) -> Result<(), DomainError> {
    if count >= 10_000 {
        return Err(DomainError::invalid(
            "configuration",
            "file fact budget exceeded",
        ));
    }
    Ok(())
}

fn record(
    input: &FeatureFlagFileInput<'_>,
    kind: &str,
    key: &str,
    edge: &str,
    start: usize,
    end: usize,
) -> Result<CodeFeatureFlagRecord, DomainError> {
    let end = start
        + input.content[start..end]
            .trim_end_matches(['\r', '\n'])
            .len();
    let range = ConfigRange {
        byte_start: start,
        byte_end: end,
        line_start: line_number(&input.content[..start]),
        line_end: line_number(&input.content[..end]),
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

fn line_number(prefix: &str) -> usize {
    let bytes = prefix.as_bytes();
    1 + bytes
        .iter()
        .enumerate()
        .filter(|(i, b)| **b == b'\r' || (**b == b'\n' && (*i == 0 || bytes[*i - 1] != b'\r')))
        .count()
}

pub(super) fn metadata(input: &FeatureFlagFileInput<'_>, start: usize) -> CodeConfigMetadata {
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
    let line_start = input.content[..start]
        .rfind(['\r', '\n'])
        .map_or(0, |i| i + 1);
    let prefix = &input.content[..line_start];
    let adjacent = prefix
        .strip_suffix("\r\n")
        .or_else(|| prefix.strip_suffix(['\r', '\n']))
        .unwrap_or(prefix);
    let block = if input.language_id == "java"
        && adjacent
            .rsplit(['\r', '\n'])
            .next()
            .is_some_and(|line| line.trim_end().ends_with("*/"))
    {
        prefix
            .rfind("/*")
            .filter(|begin| {
                prefix.len() - begin <= 8192
                    && prefix[*begin..].lines().count() <= 32
                    && prefix[..*begin]
                        .rsplit(['\r', '\n'])
                        .next()
                        .is_some_and(|line| line.trim().is_empty())
            })
            .map(|begin| &prefix[begin..])
    } else {
        None
    };
    let mut remaining = Some(adjacent);
    let lines = std::iter::from_fn(|| {
        let rest = remaining.take()?;
        if let Some(end) = rest.rfind(['\r', '\n']) {
            let before = &rest[..end];
            remaining = Some(if rest.as_bytes()[end] == b'\n' {
                before.strip_suffix('\r').unwrap_or(before)
            } else {
                before
            });
            Some(&rest[end + 1..])
        } else {
            Some(rest)
        }
    });
    for line in block.into_iter().chain(lines.take(3)) {
        let line = if input.language_id == "properties" {
            line.trim_matches(files::PROPERTY_WHITESPACE)
        } else {
            line.trim()
        };
        let comment = match input.language_id {
            "java" => line
                .strip_prefix("//")
                .or_else(|| line.strip_prefix("/*"))
                .map(|s| s.split("*/").next().unwrap_or(s)),
            "properties" => line.strip_prefix('#').or_else(|| line.strip_prefix('!')),
            "ini" => line.strip_prefix('#').or_else(|| line.strip_prefix(';')),
            "bash" => line.strip_prefix('#'),
            "gotemplate" => line
                .strip_prefix("{{/*")
                .or_else(|| line.strip_prefix("{{- /*"))
                .map(|s| s.split("*/").next().unwrap_or(s)),
            _ => None,
        };
        if line.is_empty() {
            break;
        }
        let Some(comment) = comment else {
            break;
        };
        if let Some((_, annotation)) = comment.split_once("@config ") {
            for part in annotation.split_whitespace() {
                if let Some((key, value)) = part.split_once('=') {
                    match key {
                        "domain" => meta.domain = Some(value.to_lowercase()),
                        "hot-reload" => meta.hot_reload = value.parse().ok(),
                        _ => {}
                    }
                }
            }
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
