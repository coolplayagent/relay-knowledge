//! Literal-aware Go template action boundaries shared by recovery and extraction.
#[derive(Clone, Copy)]
pub(in crate::code) enum Kind<'a> {
    Word(&'a str),
    Literal(&'a str),
    Open,
    Close,
    Pipe,
    Assign,
    Declare,
    Comma,
    Comment,
    Other,
}

pub(in crate::code) struct Token<'a> {
    pub kind: Kind<'a>,
    pub offset: usize,
}

// Delimiters inside Go quoted/raw strings or template comments are data.
pub(in crate::code) fn action_end(content: &str, start: usize) -> Option<usize> {
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

// The caller enforces the 64 KiB action bound; token slices borrow that action.
pub(in crate::code) fn tokens(action: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let bytes = action.as_bytes();
    let mut offset = 0;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte.is_ascii_whitespace() {
            offset += 1;
            continue;
        }
        if bytes[offset..].starts_with(b"/*") {
            let Some(end) = action[offset + 2..].find("*/") else {
                break;
            };
            tokens.push(Token {
                kind: Kind::Comment,
                offset,
            });
            offset += end + 4;
            continue;
        }
        let start = offset;
        offset += 1;
        let kind = match byte {
            b'(' => Kind::Open,
            b')' => Kind::Close,
            b'|' => Kind::Pipe,
            b'=' => Kind::Assign,
            b':' if bytes.get(offset) == Some(&b'=') => {
                offset += 1;
                Kind::Declare
            }
            b',' => Kind::Comma,
            b'-' => Kind::Other,
            b'"' | b'`' | b'\'' => {
                while offset < bytes.len() {
                    let next = bytes[offset];
                    offset += 1;
                    if next == b'\\' && byte != b'`' {
                        offset = (offset + 1).min(bytes.len());
                    } else if next == byte {
                        break;
                    }
                }
                Kind::Literal(&action[start..offset])
            }
            _ => {
                while offset < bytes.len()
                    && !bytes[offset].is_ascii_whitespace()
                    && !matches!(
                        bytes[offset],
                        b'(' | b')' | b'|' | b'=' | b':' | b',' | b'"' | b'`' | b'\''
                    )
                {
                    offset += 1;
                }
                Kind::Word(&action[start..offset])
            }
        };
        tokens.push(Token {
            kind,
            offset: start,
        });
    }
    tokens
}

// The proxy grammar may accept Go-invalid names without an error node.
// Check only named-template actions here, preserving its other valid syntax.
pub(in crate::code) fn template_names_valid(content: &str) -> bool {
    let mut offset = 0;
    while let Some(relative) = content[offset..].find("{{") {
        let start = offset + relative;
        let Some(end) = action_end(content, start) else {
            return false;
        };
        if end - start > 65_536 {
            return false;
        }
        let mut action = &content[start + 2..end - 2];
        if action.as_bytes().first() == Some(&b'-')
            && action
                .as_bytes()
                .get(1)
                .is_some_and(u8::is_ascii_whitespace)
        {
            action = &action[1..];
        }
        let tokens = tokens(action);
        if matches!(
            tokens.first().map(|token| token.kind),
            Some(Kind::Word("template" | "define" | "block"))
        ) && !template_name(&tokens)
        {
            return false;
        }
        offset = end;
    }
    true
}

fn template_name(tokens: &[Token<'_>]) -> bool {
    matches!(tokens.get(1).map(|token| token.kind), Some(Kind::Literal(literal))
        if super::template_literals::bytes(literal).is_some_and(|(_, rest)| rest.is_empty()))
}

pub(super) fn balanced(content: &str) -> bool {
    let mut offset = 0;
    let mut blocks = Vec::new();
    while let Some(relative) = content[offset..].find("{{") {
        let start = offset + relative;
        if content[offset..start].contains("}}") {
            return false;
        }
        let Some(end) = action_end(content, start) else {
            return false;
        };
        if end - start > 65_536 {
            return false;
        }
        let mut action = &content[start + 2..end - 2];
        if action.starts_with("- ")
            || action.starts_with("-\t")
            || action.starts_with("-\r")
            || action.starts_with("-\n")
        {
            action = &action[1..];
        }
        if action.ends_with(" -")
            || action.ends_with("\t-")
            || action.ends_with("\r-")
            || action.ends_with("\n-")
        {
            action = &action[..action.len() - 1];
        }
        let action = action.trim();
        let tokens = tokens(action);
        if !valid_action(&tokens, &mut blocks) {
            return false;
        }
        offset = end;
    }
    blocks.is_empty() && !content[offset..].contains("}}")
}

fn valid_action(tokens: &[Token<'_>], blocks: &mut Vec<(String, bool)>) -> bool {
    let Some(first) = tokens.first() else {
        return false;
    };
    if matches!(first.kind, Kind::Comment) {
        return tokens.len() == 1;
    }
    if tokens
        .iter()
        .any(|token| matches!(token.kind, Kind::Comment))
    {
        return false;
    }
    let mut expression_start = 0;
    if let Kind::Word(word) = first.kind {
        match word {
            "if" | "with" | "range" | "define" | "block" => {
                if tokens.len() < 2 || blocks.len() >= 128 {
                    return false;
                }
                if matches!(word, "define" | "block") && !template_name(tokens) {
                    return false;
                }
                if (word == "define" && (!blocks.is_empty() || tokens.len() != 2))
                    || (word == "block" && tokens.len() < 3)
                {
                    return false;
                }
                blocks.push((word.to_owned(), false));
                expression_start = 1;
            }
            "template" => {
                if !template_name(tokens) {
                    return false;
                }
                if tokens.len() == 2 {
                    return true;
                }
                expression_start = 2;
            }
            "end" => return tokens.len() == 1 && blocks.pop().is_some(),
            "else" => {
                let Some((kind, seen_else)) = blocks.last_mut() else {
                    return false;
                };
                if *seen_else || !matches!(kind.as_str(), "if" | "with" | "range") {
                    return false;
                }
                if tokens.len() == 1 {
                    *seen_else = true;
                    return true;
                }
                if tokens.len() < 3
                    || !matches!(tokens[1].kind, Kind::Word(word) if word == kind.as_str() && matches!(word, "if" | "with"))
                {
                    return false;
                }
                expression_start = 2;
            }
            "break" | "continue" => {
                return tokens.len() == 1 && blocks.iter().any(|(kind, _)| kind == "range");
            }
            _ => {}
        }
    }
    // Declaration heads bind names without evaluating those tokens. Go range
    // permits two names; other pipelines permit one. Variable uses still need
    // lexical-scope evidence beyond this conservative recovery proof.
    match &tokens[expression_start..] {
        [
            Token {
                kind: Kind::Word(first),
                ..
            },
            Token {
                kind: Kind::Comma, ..
            },
            Token {
                kind: Kind::Word(second),
                ..
            },
            Token {
                kind: Kind::Declare,
                ..
            },
            ..,
        ] if expression_start == 1 && matches!(tokens[0].kind, Kind::Word("range")) => {
            if !declaration_name(first) || !declaration_name(second) {
                return false;
            }
            expression_start += 4;
        }
        [
            Token {
                kind: Kind::Word(name),
                ..
            },
            Token {
                kind: Kind::Declare,
                ..
            },
            ..,
        ] => {
            if !declaration_name(name) {
                return false;
            }
            expression_start += 2;
        }
        _ => {}
    }
    let mut parentheses = 0usize;
    let mut need_operand = true;
    for (relative, token) in tokens[expression_start..].iter().enumerate() {
        let index = relative + expression_start;
        match token.kind {
            Kind::Open => {
                parentheses += 1;
                if parentheses > 128 {
                    return false;
                }
                need_operand = true;
            }
            Kind::Close => {
                if parentheses == 0 || need_operand {
                    return false;
                }
                parentheses -= 1;
                need_operand = false;
            }
            Kind::Pipe => {
                if need_operand
                    || !matches!(
                        tokens.get(index + 1).map(|token| token.kind),
                        Some(Kind::Word(_))
                    )
                {
                    return false;
                }
                need_operand = true;
            }
            Kind::Assign => {
                if index == 0
                    || !matches!(tokens[index-1].kind, Kind::Word(name) if name.starts_with('$'))
                {
                    return false;
                }
                need_operand = true;
            }
            Kind::Literal(literal) => {
                if super::template_literals::bytes(literal).is_none() {
                    return false;
                }
                need_operand = false;
            }
            Kind::Word(word) => {
                // Recovery does not prove variable declarations or their lexical scope.
                if word.contains('$') || !super::template_words::valid(word) {
                    return false;
                }
                need_operand = false;
            }
            Kind::Other | Kind::Declare | Kind::Comma => return false,
            Kind::Comment => return false,
        }
    }
    parentheses == 0 && !need_operand
}

fn declaration_name(name: &str) -> bool {
    name.strip_prefix('$')
        .is_some_and(|name| name.chars().all(|ch| ch == '_' || ch.is_alphanumeric()))
}

#[cfg(test)]
#[path = "template_actions_tests.rs"]
mod tests;
