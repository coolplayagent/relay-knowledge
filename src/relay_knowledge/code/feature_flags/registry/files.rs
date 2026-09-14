//! Properties/INI/template values and exported shell configuration facts.
use super::*;
use crate::code::config_files::template_action_end as action_end;
mod go_strings;
pub(super) const PROPERTY_WHITESPACE: [char; 3] = [' ', '\t', '\u{c}'];
mod pipelines;

pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    if input.language_id == "bash" {
        return super::shell::extract(input);
    }
    let mut records = Vec::new();
    let mut offset = 0;
    let mut start = 0;
    let mut logical = String::new();
    let mut section = String::new();
    let mut comment_end = 0;
    let mut segments = input.content.split_inclusive(['\r', '\n']).peekable();
    while let Some(segment) = segments.next() {
        if logical.is_empty() {
            start = offset;
        }
        let line = segment.trim_end_matches(['\r', '\n']);
        let line = if input.language_id == "gotemplate" {
            template_output_line(input.content, offset, offset + line.len(), &mut comment_end)?
        } else {
            line.to_owned()
        };
        offset += segment.len();
        if segment.ends_with('\r') && segments.peek() == Some(&"\n") {
            segments.next();
            offset += 1;
        }
        logical.push_str(if logical.is_empty() {
            &line
        } else {
            line.trim_start_matches(PROPERTY_WHITESPACE)
        });
        if input.language_id == "gotemplate" && comment_end > offset {
            continue;
        }
        if input.language_id == "properties"
            && !logical
                .trim_start_matches(PROPERTY_WHITESPACE)
                .starts_with(['#', '!'])
            && logical.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1
        {
            logical.pop();
            if offset < input.content.len() {
                continue;
            }
        }
        let line = if input.language_id == "properties" {
            logical.trim_start_matches(PROPERTY_WHITESPACE)
        } else {
            logical.trim()
        };
        if input.language_id == "ini" && line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_owned();
        } else if !line.is_empty()
            && !line.starts_with('#')
            && !(input.language_id == "properties" && line.starts_with('!'))
            && !(input.language_id == "ini" && line.starts_with(';'))
            && !line.starts_with("{{")
        {
            if let Some((key, raw)) = assignment(line, input.language_id == "properties") {
                let key = if input.language_id == "properties" {
                    decode(key).unwrap_or_default()
                } else {
                    key.to_owned()
                };
                if !key.is_empty() {
                    let key = if section.is_empty() {
                        key
                    } else {
                        format!("{section}.{key}")
                    };
                    let template = input.language_id == "gotemplate" && raw.contains("{{");
                    check_fact_budget(records.len())?;
                    let mut row = record(
                        input,
                        "config_key",
                        &key,
                        if template {
                            "declares_config_key"
                        } else {
                            "defines_config"
                        },
                        start,
                        offset,
                    )?;
                    if !template {
                        let value = if input.language_id == "properties" {
                            decode(raw).unwrap_or_else(|| raw.to_owned())
                        } else {
                            raw.trim().to_owned()
                        };
                        set_default(&mut row.metadata, value);
                    }
                    records.push(row);
                }
            }
        }
        logical.clear();
    }
    if input.language_id == "gotemplate" {
        template_reads(input, &mut records)?;
    }
    Ok(records)
}

fn assignment(line: &str, properties: bool) -> Option<(&str, &str)> {
    if line.is_empty() {
        return None;
    }
    let mut escape = false;
    for (index, ch) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if properties && ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '=' || ch == ':' || (properties && PROPERTY_WHITESPACE.contains(&ch)) {
            let whitespace = |ch: char| {
                if properties {
                    PROPERTY_WHITESPACE.contains(&ch)
                } else {
                    ch.is_whitespace()
                }
            };
            let tail = line[index..].trim_start_matches(whitespace);
            let tail = tail
                .strip_prefix(['=', ':'])
                .unwrap_or(tail)
                .trim_start_matches(whitespace);
            return Some((
                if properties {
                    &line[..index]
                } else {
                    line[..index].trim()
                },
                tail,
            ));
        }
    }
    // A bare properties key is an explicit empty string.
    properties.then_some((line, ""))
}

pub(super) fn decode(raw: &str) -> Option<String> {
    let mut units = Vec::new();
    let mut chars = raw.chars();
    while let Some(mut ch) = chars.next() {
        if ch == '\\' {
            ch = chars.next()?;
            if ch == 'u' {
                let mut value = 0_u16;
                for _ in 0..4 {
                    value = value
                        .checked_mul(16)?
                        .checked_add(chars.next()?.to_digit(16)? as u16)?;
                }
                units.push(value);
                continue;
            }
            ch = match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'f' => '\u{c}',
                other => other,
            };
        }
        let mut buffer = [0; 2];
        units.extend_from_slice(ch.encode_utf16(&mut buffer));
    }
    String::from_utf16(&units).ok()
}

fn template_reads(
    input: &FeatureFlagFileInput<'_>,
    rows: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    let mut offset = 0;
    while let Some(relative) = input.content[offset..].find("{{") {
        let start = offset + relative;
        let Some(end) = action_end(input.content, start + 2) else {
            break;
        };
        let raw = &input.content[start + 2..end];
        let action = raw.trim_start().trim_start_matches('-').trim_start();
        if !action.starts_with("/*") {
            pipelines::extract(input, action, start + 2 + raw.len() - action.len(), rows)?;
        }
        offset = end + 2;
    }
    Ok(())
}
pub(super) fn quoted(raw: &str) -> Option<(String, usize)> {
    let quote = raw.chars().next()?;
    if !matches!(quote, '"' | '`' | '\'') {
        return None;
    }
    let mut escaped = false;
    for (index, ch) in raw[1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote == '"' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some((
                if quote == '"' {
                    go_strings::decode(&raw[1..index + 1])?
                } else {
                    raw[1..index + 1].replace('\r', "")
                },
                index + 2,
            ));
        }
    }
    None
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

fn template_output_line(
    content: &str,
    start: usize,
    end: usize,
    comment_end: &mut usize,
) -> Result<String, DomainError> {
    let mut begin = start.max(*comment_end).min(end);
    let mut output = String::new();
    for _ in 0..32 {
        let Some(relative) = content[begin..end].find("{{") else {
            output.push_str(&content[begin..end]);
            return Ok(output);
        };
        let open = begin + relative;
        output.push_str(&content[begin..open]);
        let action = &content[open + 2..];
        let is_comment = action
            .trim_start()
            .trim_start_matches('-')
            .trim_start()
            .starts_with("/*");
        let Some(close) = action_end(content, open + 2) else {
            if is_comment {
                *comment_end = content.len();
            } else {
                output.push_str(&content[open..end]);
            }
            return Ok(output);
        };
        if !is_comment {
            let mut words = content[open + 2..close]
                .trim_start_matches('-')
                .split_whitespace();
            let assignment = words.next().is_some_and(|word| word.starts_with('$'))
                && words.next().is_some_and(|word| matches!(word, ":=" | "="));
            if !assignment {
                output.push_str("{{}}");
            }
            *comment_end = close + 2;
            begin = (*comment_end).min(end);
            continue;
        }
        if close - open > 8192 {
            return Err(DomainError::invalid(
                "configuration",
                "template comment byte budget exceeded",
            ));
        }
        if action.starts_with('-') {
            output.truncate(output.trim_end().len());
        }
        *comment_end = close + 2;
        begin = (*comment_end).min(end);
        if content[open..close].trim_end().ends_with('-') {
            begin = end - content[begin..end].trim_start().len();
        }
    }
    if content[begin..end].contains("{{") {
        return Err(DomainError::invalid(
            "configuration",
            "template action count budget exceeded",
        ));
    }
    output.push_str(&content[begin..end]);
    Ok(output)
}
