//! Bounded lexical shell bindings distinguish local values from environment evidence.
use tree_sitter::Node;

const MAX_BINDING_NODES: usize = 1024;

pub(super) fn environment_read(mut node: Node<'_>, key: &str, content: &str) -> bool {
    let mut assigned = false;
    let mut remaining = MAX_BINDING_NODES;
    while let Some(parent) = node.parent() {
        if remaining == 0 {
            return false;
        }
        remaining -= 1;
        if matches!(
            parent.kind(),
            "program" | "compound_statement" | "do_group" | "list"
        ) {
            let mut previous = node.prev_named_sibling();
            while let Some(statement) = previous {
                if remaining == 0 {
                    return false;
                }
                remaining -= 1;
                match binding(statement, key, content) {
                    Some(Binding::Exported) => return true,
                    Some(Binding::Local) => return false,
                    Some(Binding::Assigned) => assigned = true,
                    None => {}
                }
                previous = statement.prev_named_sibling();
            }
        }
        node = parent;
    }
    !assigned
}

enum Binding {
    Assigned,
    Local,
    Exported,
}

fn binding(node: Node<'_>, key: &str, content: &str) -> Option<Binding> {
    if node.kind() == "variable_assignment" {
        return assignment_names(node, key, content).then_some(Binding::Assigned);
    }
    if !matches!(node.kind(), "declaration_command" | "unset_command") {
        return None;
    }
    let source = content.get(node.byte_range())?;
    let mut cursor = node.walk();
    let names_key = node.named_children(&mut cursor).any(|child| {
        assignment_names(child, key, content)
            || (matches!(child.kind(), "variable_name" | "word")
                && content.get(child.byte_range()) == Some(key))
    });
    if !names_key {
        return None;
    }
    let mut words = source.split_whitespace();
    let command = words.next()?;
    let options = words
        .take_while(|word| word.starts_with(['-', '+']))
        .collect::<Vec<_>>();
    if options
        .iter()
        .any(|option| option.starts_with('+') && option.contains('x'))
        || (command == "export" && options.contains(&"-n"))
    {
        return Some(Binding::Local);
    }
    if command == "export"
        || options
            .iter()
            .any(|option| option.starts_with('-') && option.contains('x'))
    {
        return Some(Binding::Exported);
    }
    if matches!(command, "local" | "unset") {
        return Some(Binding::Local);
    }
    Some(Binding::Assigned)
}

fn assignment_names(node: Node<'_>, key: &str, content: &str) -> bool {
    node.kind() == "variable_assignment"
        && node
            .child_by_field_name("name")
            .is_some_and(|name| content.get(name.byte_range()) == Some(key))
}

#[cfg(test)]
#[path = "shell_bindings_tests.rs"]
mod tests;
