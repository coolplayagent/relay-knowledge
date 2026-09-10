//! Bounded lexical allexport state; conditional and deferred changes are not assumed.
use tree_sitter::Node;
pub(super) fn allexport(mut node: Node<'_>, content: &str) -> bool {
    let mut budget = 1024usize;
    while let Some(parent) = node.parent() {
        if parent.kind() == "function_definition" {
            return false;
        }
        let mut previous = node.prev_named_sibling();
        while let Some(statement) = previous {
            let mut pending = vec![(statement, false)];
            while let Some((candidate, conditional)) = pending.pop() {
                let Some(remaining) = budget.checked_sub(1) else {
                    return false;
                };
                budget = remaining;
                if let Some(mode) = mode(candidate, content) {
                    return !conditional && mode;
                }
                let conditional = match candidate.kind() {
                    "compound_statement" => conditional,
                    "list" | "if_statement" | "elif_clause" | "else_clause" | "while_statement"
                    | "for_statement" | "do_group" | "case_statement" | "case_item" => true,
                    _ => continue,
                };
                let mut cursor = candidate.walk();
                for child in candidate.named_children(&mut cursor) {
                    let Some(remaining) = budget.checked_sub(1) else {
                        return false;
                    };
                    budget = remaining;
                    pending.push((child, conditional));
                }
            }
            previous = statement.prev_named_sibling();
        }
        node = parent;
    }
    false
}
fn mode(node: Node<'_>, content: &str) -> Option<bool> {
    if node.kind() != "command" {
        return None;
    }
    let mut words = content[node.byte_range()].split_whitespace();
    if words.next() != Some("set") {
        return None;
    }
    let mut mode = None;
    while let Some(word) = words.next() {
        if word == "--" {
            break;
        }
        if matches!(word, "-o" | "+o") {
            if words.next() == Some("allexport") {
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
    mode
}
