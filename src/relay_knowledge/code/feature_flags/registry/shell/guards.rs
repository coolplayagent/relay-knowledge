//! Predicate ownership for shell expansions, bounded by ancestor and child budgets.
use super::*;

pub(super) fn sites(mut node: Node<'_>) -> Result<Vec<Node<'_>>, DomainError> {
    let mut budget = 1024_usize;
    while let Some(parent) = node.parent() {
        budget = budget.checked_sub(1).ok_or_else(|| {
            DomainError::invalid(
                "configuration",
                "shell guard analysis incomplete: node budget exceeded",
            )
        })?;
        if matches!(parent.kind(), "function_definition" | "program") {
            break;
        }
        if matches!(
            parent.kind(),
            "if_statement"
                | "while_statement"
                | "elif_clause"
                | "for_statement"
                | "c_style_for_statement"
                | "case_statement"
                | "list"
        ) {
            let mut before_then = true;
            for index in 0..parent.child_count() {
                budget = budget.checked_sub(1).ok_or_else(|| {
                    DomainError::invalid(
                        "configuration",
                        "shell guard analysis incomplete: node budget exceeded",
                    )
                })?;
                let child = parent.child(index as u32).unwrap();
                if child.kind() == "then" {
                    before_then = false;
                }
                if child != node {
                    continue;
                }
                let field = parent.field_name_for_child(index as u32);
                let predicate = match parent.kind() {
                    "if_statement" | "while_statement" | "c_style_for_statement" => {
                        field == Some("condition")
                    }
                    "for_statement" | "case_statement" => field == Some("value"),
                    "elif_clause" => before_then,
                    "list" => parent.named_child(0) == Some(child),
                    _ => false,
                };
                if predicate {
                    return Ok(vec![node]);
                }
                break;
            }
        }
        node = parent;
    }
    Ok(Vec::new())
}

#[cfg(test)]
#[path = "guards_tests.rs"]
mod tests;
