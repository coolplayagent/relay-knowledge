//! Properties/INI/template values and exported shell configuration facts.
use super::*;

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
    for segment in input.content.split_inclusive('\n') {
        if logical.is_empty() {
            start = offset;
        }
        let line = segment.trim_end_matches(['\r', '\n']);
        offset += segment.len();
        logical.push_str(if logical.is_empty() {
            line
        } else {
            line.trim_start()
        });
        if input.language_id == "properties"
            && logical.chars().rev().take_while(|c| *c == '\\').count() % 2 == 1
        {
            logical.pop();
            continue;
        }
        let line = logical.trim();
        if input.language_id == "ini" && line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_owned();
        } else if !line.is_empty() && !line.starts_with(['#', '!', ';']) && !line.starts_with("{{")
        {
            if let Some((key, raw)) = assignment(line) {
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
                            decode(raw.trim()).unwrap_or_else(|| raw.to_owned())
                        } else {
                            raw.trim().to_owned()
                        };
                        row.metadata.value_type = Some(value_type(&value).to_owned());
                        row.metadata.default_value = Some(value);
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

fn assignment(line: &str) -> Option<(&str, &str)> {
    if line.is_empty() {
        return None;
    }
    let mut escape = false;
    for (index, ch) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '=' || ch == ':' || ch.is_whitespace() {
            let tail = line[index..].trim_start();
            let tail = tail.strip_prefix(['=', ':']).unwrap_or(tail).trim_start();
            return Some((line[..index].trim(), tail));
        }
    }
    // A bare properties key is an explicit empty string.
    Some((line, ""))
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
        let action = input.content[start + 2..end]
            .trim()
            .trim_matches('-')
            .trim();
        if !action.starts_with("/*") {
            let mut tokens = action.splitn(2, char::is_whitespace);
            let command = tokens.next().unwrap_or_default();
            let argument = tokens.next().unwrap_or_default().trim();
            if matches!(command, "key" | "keyOrDefault" | "env") {
                if let Some((key, _)) = quoted(argument) {
                    rows.push(record(
                        input,
                        if command == "env" {
                            "env_var"
                        } else {
                            "config_key"
                        },
                        &key,
                        "reads_config",
                        start,
                        end + 2,
                    )?);
                }
            }
        }
        offset = end + 2;
    }
    Ok(())
}
fn action_end(content: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in content[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if let Some(q) = quote {
            if ch == '\\' && q != '`' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
        } else if matches!(ch, '"' | '`') {
            quote = Some(ch);
        } else if content[start + index..].starts_with("}}") {
            return Some(start + index);
        }
    }
    None
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
                    decode(&raw[1..index + 1])?
                } else {
                    raw[1..index + 1].to_owned()
                },
                index + 2,
            ));
        }
    }
    None
}
