use tree_sitter::Node;

use super::{SyntaxRange, compact_whitespace, node_text, quoted_specifier, syntax_range};

pub(super) fn dynamic_import(
    language_id: &str,
    content: &str,
    node: Node<'_>,
) -> Option<(String, SyntaxRange)> {
    if !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        || node.kind() != "call_expression"
    {
        return None;
    }
    let function = node.child_by_field_name("function")?;
    if function.kind() != "import" {
        return None;
    }
    first_direct_string_argument(content, node.child_by_field_name("arguments")?)?;
    let source = dynamic_import_source(content, node);

    Some(source)
}

fn dynamic_import_source(content: &str, node: Node<'_>) -> (String, SyntaxRange) {
    let source_node = node
        .parent()
        .filter(|parent| parent.kind() == "await_expression")
        .unwrap_or(node);

    (
        compact_whitespace(&node_text(content, source_node)),
        syntax_range(source_node),
    )
}

fn first_direct_string_argument(content: &str, arguments: Node<'_>) -> Option<String> {
    for index in 0..arguments.named_child_count() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        let child = arguments.named_child(index)?;
        if matches!(child.kind(), "string" | "string_literal") {
            return Some(node_text(content, child));
        }
        return None;
    }

    None
}

pub(super) fn re_export(
    language_id: &str,
    content: &str,
    node: Node<'_>,
) -> Option<(String, SyntaxRange)> {
    if !matches!(language_id, "javascript" | "jsx" | "typescript" | "tsx")
        || node.kind() != "export_statement"
    {
        return None;
    }
    let statement = compact_whitespace(&node_text(content, node));
    if !export_has_module_specifier(&statement) {
        return None;
    }

    Some((statement, syntax_range(node)))
}

fn export_has_module_specifier(statement: &str) -> bool {
    let Some(body) = statement
        .trim()
        .trim_end_matches(';')
        .trim()
        .strip_prefix("export ")
    else {
        return false;
    };
    body.rsplit_once(" from ")
        .is_some_and(|(_, module)| quoted_specifier(module).is_some())
}
