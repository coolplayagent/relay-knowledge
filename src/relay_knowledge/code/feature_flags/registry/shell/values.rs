//! Decode static shell words without treating quoted dollar signs as expansions.
use crate::domain::DomainError;
use tree_sitter::Node;
/// Recognize assignments both in declaration ASTs and decoded builtin operands.
pub(super) fn assignment(
    node: Node<'_>,
    content: &str,
) -> Result<Option<(String, Option<String>)>, DomainError> {
    if node.kind() == "variable_assignment" {
        let Some(name) = node.child_by_field_name("name") else {
            return Ok(None);
        };
        let value = node.child_by_field_name("value");
        let append =
            value.is_some_and(|value| content[name.end_byte()..value.start_byte()].contains("+="));
        return Ok(Some((
            content[name.byte_range()].to_owned(),
            if append {
                None
            } else {
                static_value(value, content)?
            },
        )));
    }
    let Some(separator) = content[node.byte_range()].find('=') else {
        return Ok(None);
    };
    let decoded = decode(node, content, node.end_byte(), true)?;
    let (name, value) = if let Some(decoded) = decoded {
        let Some((name, value)) = decoded.split_once('=') else {
            return Ok(None);
        };
        (name.to_owned(), Some(value.to_owned()))
    } else {
        let Some(name) = decode(node, content, node.start_byte() + separator, true)? else {
            return Ok(None);
        };
        (name, None)
    };
    let append = name.ends_with('+');
    let name = name.strip_suffix('+').unwrap_or(&name);
    if name.is_empty()
        || name.starts_with(|c: char| c.is_ascii_digit())
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Ok(None);
    }
    Ok(Some((name.to_owned(), if append { None } else { value })))
}
pub(super) fn static_value(
    node: Option<Node<'_>>,
    content: &str,
) -> Result<Option<String>, DomainError> {
    let Some(node) = node else {
        return Ok(Some(String::new()));
    };
    decode(node, content, node.end_byte(), false)
}
/// Decode only the requested prefix when the value is dynamic; assignments have
/// a separate tilde-expansion position immediately after their first equals sign.
fn decode(
    node: Node<'_>,
    content: &str,
    end: usize,
    assignment: bool,
) -> Result<Option<String>, DomainError> {
    let mut pending = vec![node];
    let mut budget = 1024usize;
    while let Some(part) = pending.pop() {
        budget = budget.checked_sub(1).ok_or_else(|| {
            DomainError::invalid(
                "configuration",
                "shell value analysis incomplete: lexical budget exceeded",
            )
        })?;
        if part.start_byte() >= end {
            continue;
        }
        if part.kind().ends_with("expansion")
            || matches!(
                part.kind(),
                "command_substitution" | "process_substitution" | "ansi_c_string"
            )
        {
            return Ok(None);
        }
        let mut cursor = part.walk();
        pending.extend(part.named_children(&mut cursor));
    }
    let mut quote = None;
    let mut value = String::new();
    let mut chars = content[node.start_byte()..end].chars().peekable();
    while let Some(ch) = chars.next() {
        if quote.is_none()
            && ch == '~'
            && (value.is_empty()
                || value.ends_with(':')
                || (assignment && value.ends_with('=') && !value[..value.len() - 1].contains('=')))
        {
            return Ok(None);
        }
        if quote == Some('\'') {
            if ch == '\'' {
                quote = None;
            } else {
                value.push(ch);
            }
        } else if ch == '\\' {
            let Some(next) = chars.peek().copied() else {
                return Ok(None);
            };
            if quote.is_none() || matches!(next, '$' | '`' | '"' | '\\' | '\n') {
                chars.next();
                if next != '\n' {
                    value.push(next);
                }
            } else {
                value.push(ch);
            }
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else {
            value.push(ch);
        }
    }
    Ok((quote.is_none() || end < node.end_byte()).then_some(value))
}
