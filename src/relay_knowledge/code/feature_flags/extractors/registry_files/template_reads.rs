//! Whole-file, linear template action scanning with bounded literal decoding.
use crate::code::config_files::ConfigRange;
use crate::code::feature_flags::{FeatureFlagFileInput, feature_flag_record_from_range};
use crate::domain::{CodeFeatureFlagRecord, DomainError};

const MAX_ACTION_BYTES: usize = 65_536;

pub(super) fn collect(
    input: &FeatureFlagFileInput<'_>,
    records: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<String, DomainError> {
    let mut outside = input.content.as_bytes().to_vec();
    let mut offset = 0;
    let mut line = 1;
    while let Some(relative) = input.content[offset..].find("{{") {
        let start = offset + relative;
        line += input.content[offset..start]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let end = action_end(input.content, start);
        for byte in &mut outside[start..end.unwrap_or(input.content.len())] {
            if !matches!(*byte, b'\r' | b'\n') {
                *byte = b' ';
            }
        }
        let Some(end) = end else {
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
    Ok(String::from_utf8(outside).expect("complete UTF-8 action spans are replaced with ASCII"))
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
    for read in super::template_commands::reads(action) {
        let mut record = feature_flag_record_from_range(
            input,
            read.kind,
            &read.key,
            "reads_config",
            range,
            excerpt,
        )?;
        record.usage_id = crate::code::stable_id(
            "feature_flag_template_read",
            [record.usage_id.as_str(), &read.offset.to_string()],
        );
        record.metadata.source_format = "ctmpl".to_owned();
        if let Some(value) = read.default {
            record.metadata.value_type = Some(super::value_type(&value).to_owned());
            record.metadata.default_value = Some(value);
        }
        records.push(record);
    }
    Ok(())
}

#[cfg(test)]
#[path = "template_reads_tests.rs"]
mod tests;
