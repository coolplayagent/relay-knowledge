//! Bounded lexical allexport state; conditional and deferred changes are not assumed.
use crate::domain::DomainError;
use tree_sitter::Node;
pub(super) fn allexport(mut node: Node<'_>, content: &str) -> Result<(bool, bool), DomainError> {
    let mut budget = 1024usize;
    let mut uncertain = false;
    while let Some(parent) = node.parent() {
        if parent.kind() == "function_definition" {
            return Ok((false, uncertain));
        }
        let mut previous = node.prev_named_sibling();
        while let Some(statement) = previous {
            let mut pending = vec![(statement, false)];
            while let Some((candidate, conditional)) = pending.pop() {
                let Some(remaining) = budget.checked_sub(1) else {
                    return Err(DomainError::invalid(
                        "configuration",
                        "shell allexport analysis incomplete: lexical budget exceeded",
                    ));
                };
                budget = remaining;
                if candidate
                    .next_sibling()
                    .is_some_and(|next| next.kind() == "&")
                {
                    continue;
                }
                if let Some(mode) = mode(candidate, content)? {
                    if conditional {
                        uncertain = true;
                        continue;
                    }
                    return Ok((mode, uncertain));
                }
                let conditional = match candidate.kind() {
                    "compound_statement" | "list" => conditional,
                    "if_statement" | "elif_clause" | "else_clause" | "while_statement"
                    | "for_statement" | "do_group" | "case_statement" | "case_item" => true,
                    _ => continue,
                };
                let mut cursor = candidate.walk();
                for child in candidate.named_children(&mut cursor) {
                    let Some(remaining) = budget.checked_sub(1) else {
                        return Err(DomainError::invalid(
                            "configuration",
                            "shell allexport analysis incomplete: lexical budget exceeded",
                        ));
                    };
                    budget = remaining;
                    pending.push((
                        child,
                        conditional
                            || (candidate.kind() == "list"
                                && candidate.named_child(0) != Some(child)),
                    ));
                }
            }
            previous = statement.prev_named_sibling();
        }
        node = parent;
    }
    Ok((false, uncertain))
}
fn mode(node: Node<'_>, content: &str) -> Result<Option<bool>, DomainError> {
    if node.kind() != "command" {
        return Ok(None);
    }
    let mut cursor = node.walk();
    let mut words = node
        .named_children(&mut cursor)
        .filter(|child| !child.is_extra())
        .map(|word| super::values::static_value(Some(word), content));
    if words.next().transpose()?.flatten().as_deref() != Some("set") {
        return Ok(None);
    }
    let mut mode = None;
    while let Some(word) = words.next().transpose()?.flatten() {
        if word == "--" {
            break;
        }
        if matches!(word.as_str(), "-o" | "+o") {
            if words.next().transpose()?.flatten().as_deref() == Some("allexport") {
                mode = Some(word == "-o");
            }
        } else if word.starts_with(['-', '+']) {
            if word[1..].contains('a') {
                mode = Some(word.starts_with('-'));
            }
        } else {
            break;
        }
    }
    Ok(mode)
}
