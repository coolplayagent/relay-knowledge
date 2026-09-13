//! Dotenv assignment grammar with bounded values and original source locations.
use super::*;
pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut rows = Vec::new();
    let mut offset = 0;
    while offset < input.content.len() {
        let start = offset;
        let end = natural_line_end(input.content, start);
        offset = end;
        let line = input.content[start..end].trim();
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
        {
            continue;
        }
        let equals = input.content[start..end].find('=').unwrap() + start;
        let value_start = equals + 1;
        let raw = &input.content[value_start..];
        let raw = raw.trim_start_matches([' ', '\t']);
        let begin = input.content.len() - raw.len();
        let mut value = String::new();
        let mut incomplete = false;
        let mut value_end = end;
        if let Some(quote) = raw.chars().next().filter(|c| matches!(c, '\'' | '"')) {
            let mut chars = raw[1..].char_indices();
            let mut closed = false;
            while let Some((index, ch)) = chars.next() {
                if ch == quote {
                    value_end = begin + index + 2;
                    closed = true;
                    break;
                }
                let decoded = if ch == '\\' && quote == '"' {
                    match chars.next().map(|(_, c)| c) {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some(c @ ('"' | '\\' | '$')) => c,
                        Some(c) => {
                            if value.len() <= 60 * 1024 {
                                value.push('\\');
                            }
                            c
                        }
                        None => {
                            incomplete = true;
                            break;
                        }
                    }
                } else {
                    if quote == '"' && matches!(ch, '$' | '`') {
                        incomplete = true;
                    }
                    ch
                };
                if value.len() <= 60 * 1024 {
                    value.push(decoded);
                } else {
                    incomplete = true;
                }
            }
            if !closed {
                incomplete = true;
                value_end = input.content.len();
            }
            offset = natural_line_end(input.content, value_end);
            let tail = input.content[value_end..offset].trim();
            if closed && !tail.is_empty() && !tail.starts_with('#') {
                incomplete = true;
            }
        } else {
            let raw = &input.content[begin..end];
            let comment = raw
                .char_indices()
                .find(|(index, ch)| {
                    *ch == '#' && (*index == 0 || raw[..*index].ends_with([' ', '\t']))
                })
                .map_or(raw.len(), |(index, _)| index);
            let raw = raw[..comment].trim();
            if raw.len() > 60 * 1024 || raw.contains(['$', '`']) {
                incomplete = true;
            } else {
                value.push_str(raw);
            }
        }
        check_fact_budget(rows.len())?;
        let mut row = record(input, "env_var", key, "defines_config", start, offset)?;
        if incomplete {
            row.metadata.flow_incomplete = Some("dotenv_value_incomplete".into());
        } else {
            set_default(&mut row.metadata, value);
        }
        rows.push(row);
    }
    Ok(rows)
}
#[cfg(test)]
#[path = "dotenv_tests.rs"]
mod tests;

fn natural_line_end(content: &str, start: usize) -> usize {
    let Some(relative) = content[start..].find(['\r', '\n']) else {
        return content.len();
    };
    let end = start + relative + 1;
    if content.as_bytes()[end - 1] == b'\r' && content.as_bytes().get(end) == Some(&b'\n') {
        end + 1
    } else {
        end
    }
}
