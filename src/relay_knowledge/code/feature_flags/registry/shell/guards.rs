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
mod tests {
    use super::*;

    #[test]
    fn wide_shell_predicates_fail_explicitly_when_guard_budget_is_exhausted() {
        let source = format!("if {}test \"$FEATURE\"; then :; fi", "true; ".repeat(1100));
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let start = source.find("$FEATURE").unwrap();
        let expansion = tree
            .root_node()
            .named_descendant_for_byte_range(start, start + 8)
            .unwrap();
        assert!(
            sites(expansion)
                .unwrap_err()
                .to_string()
                .contains("shell guard analysis incomplete")
        );
    }
}
