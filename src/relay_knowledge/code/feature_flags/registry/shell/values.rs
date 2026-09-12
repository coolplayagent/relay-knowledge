//! Decode static shell words without treating quoted dollar signs as expansions.
use crate::domain::DomainError;
use tree_sitter::Node;
pub(super) fn static_value(
    node: Option<Node<'_>>,
    content: &str,
) -> Result<Option<String>, DomainError> {
    let Some(node) = node else {
        return Ok(Some(String::new()));
    };
    let mut pending = vec![node];
    let mut budget = 1024usize;
    while let Some(part) = pending.pop() {
        budget = budget.checked_sub(1).ok_or_else(|| {
            DomainError::invalid(
                "configuration",
                "shell value analysis incomplete: lexical budget exceeded",
            )
        })?;
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
    let mut chars = content[node.byte_range()].chars().peekable();
    while let Some(ch) = chars.next() {
        if quote.is_none() && ch == '~' && (value.is_empty() || value.ends_with(':')) {
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
    Ok(quote.is_none().then_some(value))
}
