//! Bounded template expression tokens, preserving literal and call-site boundaries.
use super::*;
enum Token<'a> {
    Word(&'a str, usize),
    Literal(String, usize),
    Boundary(char),
}
pub(super) fn extract(
    input: &FeatureFlagFileInput<'_>,
    action: &str,
    start: usize,
    rows: &mut Vec<CodeFeatureFlagRecord>,
) -> Result<(), DomainError> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < action.len() {
        let tail = &action[offset..];
        let ch = tail.chars().next().unwrap();
        if ch.is_whitespace() {
            offset += ch.len_utf8();
            continue;
        }
        if tokens.len() >= 256 {
            return Err(DomainError::invalid(
                "configuration",
                "template pipeline analysis incomplete: token budget exceeded",
            ));
        }
        if matches!(ch, '"' | '`') {
            let Some((value, consumed)) = quoted(tail) else {
                break;
            };
            offset += consumed;
            tokens.push(Token::Literal(value, offset));
        } else if matches!(ch, '(' | ')' | '|') {
            offset += ch.len_utf8();
            tokens.push(Token::Boundary(ch));
        } else {
            let len = tail
                .char_indices()
                .find(|(_, c)| c.is_whitespace() || matches!(c, '(' | ')' | '|' | '"' | '`'))
                .map_or(tail.len(), |(i, _)| i);
            tokens.push(Token::Word(&tail[..len], offset));
            offset += len;
        }
    }
    let mut command_position = true;
    for (index, token) in tokens.iter().enumerate() {
        if let Token::Boundary(ch) = token {
            command_position = matches!(ch, '(' | '|');
            continue;
        }
        let Token::Word(command, begin) = token else {
            command_position = false;
            continue;
        };
        let first = command_position;
        command_position = (first && matches!(*command, "if" | "with" | "range" | "else"))
            || matches!(*command, ":=" | "=");
        if !first {
            continue;
        }
        if !matches!(*command, "key" | "keyOrDefault" | "env") {
            continue;
        }
        let Some(Token::Literal(key, end)) = tokens.get(index + 1) else {
            continue;
        };
        let fallback = if *command == "keyOrDefault" {
            tokens.get(index + 2)
        } else {
            None
        };
        let end = if let Some(Token::Literal(_, end)) = fallback {
            *end
        } else {
            *end
        };
        check_fact_budget(rows.len())?;
        let mut row = record(
            input,
            if *command == "env" {
                "env_var"
            } else {
                "config_key"
            },
            key,
            "reads_config",
            start + begin,
            start + end,
        )?;
        if let Some(Token::Literal(value, _)) = fallback {
            row.metadata.default_value = Some(value.clone());
            row.metadata.value_type = Some(value_type(value).into());
        }
        rows.push(row);
    }
    Ok(())
}
