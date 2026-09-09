//! Whole-file, linear template action scanning with bounded literal decoding.
use super::template_literals::string as template_string;
use crate::code::config_files::ConfigRange;
use crate::code::feature_flags::{FeatureFlagFileInput, feature_flag_record_from_range};
use crate::domain::{CodeFeatureFlagRecord, DomainError};

const MAX_ACTION_BYTES: usize = 65_536;

pub(super) fn collect(
    input: &FeatureFlagFileInput<'_>,
    records: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    let mut offset = 0;
    let mut line = 1;
    while let Some(relative) = input.content[offset..].find("{{") {
        let start = offset + relative;
        line += input.content[offset..start]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let Some(end) = action_end(input.content, start) else {
            break;
        };
        let excerpt = &input.content[start..end];
        let end_line = line + excerpt.bytes().filter(|b| *b == b'\n').count();
        if excerpt.len() > MAX_ACTION_BYTES {
            return Err(DomainError::invalid(
                "configuration_template_action",
                "template action exceeds the 65536-byte literal decoding budget",
            ));
        }
        let range = ConfigRange {
            byte_start: start,
            byte_end: end,
            line_start: line,
            line_end: end_line,
        };
        collect_action(
            input,
            &excerpt[2..excerpt.len() - 2],
            range,
            excerpt,
            records,
        )?;
        offset = end;
        line = end_line;
    }
    Ok(())
}

// Delimiters inside Go quoted/raw strings or template comments are data.
fn action_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut index = start + 2;
    let mut quote = None;
    let mut comment = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if comment {
            if bytes[index..].starts_with(b"*/") {
                comment = false;
                index += 2;
                continue;
            }
        } else if let Some(delimiter) = quote {
            if byte == b'\\' && delimiter != b'`' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if byte == delimiter {
                quote = None;
            }
        } else if bytes[index..].starts_with(b"}}") {
            return Some(index + 2);
        } else if bytes[index..].starts_with(b"/*") {
            comment = true;
            index += 2;
            continue;
        } else if matches!(byte, b'"' | b'\'' | b'`') {
            quote = Some(byte);
        }
        index += 1;
    }
    None
}

fn collect_action(
    input: &FeatureFlagFileInput<'_>,
    action: &str,
    range: ConfigRange,
    excerpt: &str,
    records: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    let action = action.trim().trim_start_matches('-').trim();
    let (function, arguments) = action
        .split_once(char::is_whitespace)
        .unwrap_or((action, ""));
    let kind = match function {
        "key" | "keyOrDefault" => "config_key",
        "env" => "env_var",
        _ => return Ok(()),
    };
    let Some((key, arguments)) = template_string(arguments) else {
        return Ok(());
    };
    if !super::valid_key(&key) {
        return Ok(());
    }
    let mut record =
        feature_flag_record_from_range(input, kind, &key, "reads_config", range, excerpt)?;
    record.metadata.source_format = "ctmpl".to_owned();
    if function == "keyOrDefault" {
        if let Some((value, _)) = template_string(arguments) {
            record.metadata.value_type = Some(super::value_type(&value).to_owned());
            record.metadata.default_value = Some(value);
        }
    }
    records.push(record);
    Ok(())
}

#[cfg(test)]
#[path = "template_reads_tests.rs"]
mod tests;
