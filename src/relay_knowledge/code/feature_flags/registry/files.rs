//! Properties/INI/template values and exported shell configuration facts.
use super::*;
use tree_sitter::Node;

pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    if input.language_id == "bash" {
        return shell(input);
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
                let key = decode(key).unwrap_or_default();
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
                        let value = decode(raw.trim()).unwrap_or_else(|| raw.to_owned());
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
fn quoted(raw: &str) -> Option<(String, usize)> {
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

fn shell(input: &FeatureFlagFileInput<'_>) -> Result<Vec<CodeFeatureFlagRecord>, DomainError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .map_err(|e| DomainError::invalid("shell", e.to_string()))?;
    let tree = parser
        .parse(input.content, None)
        .ok_or_else(|| DomainError::invalid("shell", "parse cancelled"))?;
    let mut pending = vec![tree.root_node()];
    let mut rows = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "variable_assignment"
            && node
                .parent()
                .and_then(|parent| export_mode(parent, input.content))
                == Some(true)
        {
            if let (Some(name), Some(value)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            ) {
                let mut row = record(
                    input,
                    "env_var",
                    &input.content[name.byte_range()],
                    "defines_config",
                    node.start_byte(),
                    node.end_byte(),
                )?;
                let value = &input.content[value.byte_range()];
                if !value.contains(['$', '`']) {
                    let value = quoted(value).map_or_else(|| value.to_owned(), |(value, _)| value);
                    row.metadata.value_type = Some(value_type(&value).to_owned());
                    row.metadata.default_value = Some(value);
                }
                rows.push(row);
            }
        }
        if matches!(node.kind(), "simple_expansion" | "expansion") {
            let mut cursor = node.walk();
            if let Some(name) = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "variable_name")
            {
                let key = &input.content[name.byte_range()];
                if shell_external(node, key, input.content) {
                    rows.push(record(
                        input,
                        "env_var",
                        key,
                        "reads_config",
                        node.start_byte(),
                        node.end_byte(),
                    )?);
                }
            }
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    Ok(rows)
}

fn export_mode(node: Node<'_>, content: &str) -> Option<bool> {
    if !matches!(node.kind(), "declaration_command" | "unset_command") {
        return None;
    }
    let mut words = content[node.byte_range()].split_whitespace();
    let command = words.next()?;
    let options = words
        .take_while(|word| word.starts_with(['-', '+']))
        .collect::<Vec<_>>();
    if command == "unset"
        || options
            .iter()
            .any(|option| *option == "-n" || (option.starts_with('+') && option.contains('x')))
    {
        return Some(false);
    }
    if options.contains(&"-p") {
        return None;
    }
    if command == "export"
        || options
            .iter()
            .any(|option| option.starts_with('-') && option.contains('x'))
    {
        return Some(true);
    }
    (command == "local").then_some(false)
}
fn shell_external(mut node: Node<'_>, key: &str, content: &str) -> bool {
    let mut budget = 1024_usize;
    let mut assigned = false;
    while let Some(parent) = node.parent() {
        if budget == 0 {
            return false;
        }
        budget -= 1;
        if matches!(parent.kind(), "program" | "compound_statement" | "do_group") {
            let mut previous = node.prev_named_sibling();
            while let Some(statement) = previous {
                let mut pending = vec![(statement, false)];
                while let Some((candidate, conditional)) = pending.pop() {
                    if budget == 0 {
                        return false;
                    }
                    budget -= 1;
                    if let Some(exported) = export_mode(candidate, content) {
                        let mut cursor = candidate.walk();
                        let names = candidate.named_children(&mut cursor).any(|child| {
                            (matches!(child.kind(), "word" | "variable_name")
                                && &content[child.byte_range()] == key)
                                || (child.kind() == "variable_assignment"
                                    && child
                                        .child_by_field_name("name")
                                        .is_some_and(|name| &content[name.byte_range()] == key))
                        });
                        if names {
                            return !conditional && exported;
                        }
                    }
                    if candidate.kind() == "variable_assignment"
                        && candidate
                            .child_by_field_name("name")
                            .is_some_and(|name| &content[name.byte_range()] == key)
                    {
                        if conditional {
                            return false;
                        }
                        assigned = true;
                        continue;
                    }
                    let conditional = match candidate.kind() {
                        "compound_statement" | "declaration_command" => conditional,
                        "list" | "if_statement" | "elif_clause" | "else_clause"
                        | "while_statement" | "for_statement" | "do_group" | "case_statement"
                        | "case_item" => true,
                        _ => continue,
                    };
                    let mut cursor = candidate.walk();
                    for child in candidate.named_children(&mut cursor) {
                        if budget == 0 {
                            return false;
                        }
                        budget -= 1;
                        pending.push((child, conditional));
                    }
                }
                previous = statement.prev_named_sibling();
            }
        }
        node = parent;
    }
    !assigned
}
