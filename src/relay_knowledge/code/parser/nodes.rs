use tree_sitter::Node;

#[derive(Debug, Clone)]
pub(super) struct SyntaxRange {
    pub(super) byte_start: usize,
    pub(super) byte_end: usize,
    pub(super) line_start: usize,
    pub(super) line_end: usize,
}

pub(super) fn syntax_range(node: Node<'_>) -> SyntaxRange {
    SyntaxRange {
        byte_start: node.start_byte(),
        byte_end: node.end_byte(),
        line_start: node.start_position().row + 1,
        line_end: node.end_position().row + 1,
    }
}

pub(super) fn node_text(content: &str, node: Node<'_>) -> String {
    node.utf8_text(content.as_bytes())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub(super) fn last_identifier_text(content: &str, node: Node<'_>) -> Option<String> {
    let mut stack = Vec::with_capacity(node.child_count().saturating_add(1));
    stack.push(node);
    let mut last = None;
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "command_name"
                | "identifier"
                | "field_identifier"
                | "property_identifier"
                | "simple_identifier"
                | "type_identifier"
                | "value_identifier"
                | "word"
        ) {
            last = Some(node_text(content, current));
            continue;
        }
        push_children_reverse(current, &mut stack);
    }

    last
}

pub(super) fn push_children_reverse<'tree>(node: Node<'tree>, stack: &mut Vec<Node<'tree>>) {
    for index in (0..node.child_count()).rev() {
        let Ok(index) = u32::try_from(index) else {
            continue;
        };
        if let Some(child) = node.child(index) {
            stack.push(child);
        }
    }
}

pub(super) fn first_named_child_of_kind<'tree>(
    root: Node<'tree>,
    kind: &str,
) -> Option<Node<'tree>> {
    let mut stack = Vec::new();
    push_children_reverse(root, &mut stack);
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return Some(node);
        }
        push_children_reverse(node, &mut stack);
    }

    None
}

pub(super) fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
