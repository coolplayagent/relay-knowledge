//! Bounded template expression tokens, preserving literal and call-site boundaries.
use super::*;
enum Token<'a> {
    Word(&'a str, usize),
    Literal(String, usize, usize),
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
            tokens.push(Token::Literal(value, offset - consumed, offset));
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
        let incoming = if index >= 2 && matches!(tokens[index - 1], Token::Boundary('|')) {
            match &tokens[index - 2] {
                token @ Token::Literal(..)
                    if index == 2
                        || matches!(
                            tokens[index - 3],
                            Token::Boundary('(' | '|')
                                | Token::Word("if" | "with" | "range" | ":=" | "=", _)
                        ) =>
                {
                    Some(token)
                }
                _ => None,
            }
        } else {
            None
        };
        let explicit = literal_argument(&tokens, index + 1);
        let key_token = match explicit {
            Some((token, _)) => Some(token),
            _ if *command != "keyOrDefault" => incoming,
            _ => None,
        };
        let Some(Token::Literal(key, key_begin, key_end)) = key_token else {
            continue;
        };
        let fallback = if *command == "keyOrDefault" {
            match explicit.and_then(|(_, next)| literal_argument(&tokens, next)) {
                Some((token, _)) => Some(token),
                _ => incoming,
            }
        } else {
            None
        };
        let mut begin = (*begin).min(*key_begin);
        let mut end = (*key_end).max(match token {
            Token::Word(_, offset) => *offset + command.len(),
            _ => *key_end,
        });
        if let Some(Token::Literal(_, fallback_begin, fallback_end)) = fallback {
            begin = begin.min(*fallback_begin);
            end = end.max(*fallback_end);
        }
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
        if let Some(Token::Literal(value, _, _)) = fallback {
            set_default(&mut row.metadata, value.clone());
        } else if *command == "keyOrDefault" {
            row.metadata.flow_incomplete = Some("dynamic_template_fallback".into());
        }
        rows.push(row);
    }
    Ok(())
}

fn literal_argument<'a, 'b>(
    tokens: &'a [Token<'b>],
    mut index: usize,
) -> Option<(&'a Token<'b>, usize)> {
    let mut depth = 0;
    while matches!(tokens.get(index), Some(Token::Boundary('('))) {
        depth += 1;
        if depth > 32 {
            return None;
        }
        index += 1;
    }
    let token = tokens
        .get(index)
        .filter(|t| matches!(t, Token::Literal(..)))?;
    index += 1;
    for _ in 0..depth {
        if !matches!(tokens.get(index), Some(Token::Boundary(')'))) {
            return None;
        }
        index += 1;
    }
    Some((token, index))
}
