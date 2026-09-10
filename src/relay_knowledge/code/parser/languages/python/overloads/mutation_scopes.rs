//! Resolve redirected class writes without treating class locals as closure cells.
use crate::code::parser::nodes::node_text;
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Unbound,
    Local,
    Global,
    Nonlocal,
}

pub(super) fn redirects_to_origin(
    content: &str,
    class: Node<'_>,
    origin: Option<Node<'_>>,
    name: &str,
    remaining: &mut usize,
) -> bool {
    let directive = scope_binding(content, class, name, usize::MAX, remaining);
    let destination = match directive {
        Some(Binding::Global) => module_scope(class, remaining),
        Some(Binding::Nonlocal) => owner(
            content,
            parent_scope(class, remaining),
            name,
            usize::MAX,
            remaining,
        ),
        Some(Binding::Local | Binding::Unbound) => return false,
        None => return true,
    };
    let origin = owner(content, origin, name, class.start_byte(), remaining);
    destination.is_none() || origin.is_none() || destination == origin
}

fn owner<'a>(
    content: &str,
    mut scope: Option<Node<'a>>,
    name: &str,
    before: usize,
    remaining: &mut usize,
) -> Option<Node<'a>> {
    while let Some(current) = scope {
        if current.kind() == "module" {
            return Some(current);
        }
        match scope_binding(content, current, name, before, remaining)? {
            Binding::Local => return Some(current),
            Binding::Global => return module_scope(current, remaining),
            Binding::Nonlocal | Binding::Unbound => {}
        }
        scope = parent_scope(current, remaining);
    }
    None
}

fn parent_scope<'a>(mut node: Node<'a>, remaining: &mut usize) -> Option<Node<'a>> {
    while let Some(parent) = node.parent() {
        *remaining = remaining.checked_sub(1)?;
        if matches!(parent.kind(), "function_definition" | "lambda" | "module") {
            return Some(parent);
        }
        node = parent;
    }
    None
}

fn module_scope<'a>(mut node: Node<'a>, remaining: &mut usize) -> Option<Node<'a>> {
    while node.kind() != "module" {
        *remaining = remaining.checked_sub(1)?;
        node = node.parent()?;
    }
    Some(node)
}

// This is lexical declaration analysis, not execution-effect analysis: nested
// function/class bodies cannot establish a local name in their enclosing scope.
fn scope_binding(
    content: &str,
    scope: Node<'_>,
    name: &str,
    before: usize,
    remaining: &mut usize,
) -> Option<Binding> {
    let mut stack = vec![scope.child_by_field_name("body")?];
    if let Some(parameters) = scope.child_by_field_name("parameters") {
        stack.push(parameters);
    }
    let mut binding = Binding::Unbound;
    while let Some(current) = stack.pop() {
        *remaining = remaining.checked_sub(1)?;
        if matches!(current.kind(), "global_statement" | "nonlocal_statement")
            && names(content, current, name, remaining)?
        {
            return Some(if current.kind() == "global_statement" {
                Binding::Global
            } else {
                Binding::Nonlocal
            });
        }
        let visible = scope.kind() != "class_definition" || current.start_byte() < before;
        if visible
            && current.kind() == "case_clause"
            && super::pattern_bindings::binds(content, current, name, remaining)
        {
            binding = Binding::Local;
        }
        let target = match current.kind() {
            // Class annotations record metadata without installing a value;
            // function annotations still declare a local for the whole body.
            "assignment"
                if current.child_by_field_name("right").is_some()
                    || scope.kind() != "class_definition" =>
            {
                current.child_by_field_name("left")
            }
            "augmented_assignment" | "for_statement" => current.child_by_field_name("left"),
            "function_definition" | "class_definition" | "named_expression" => {
                current.child_by_field_name("name")
            }
            "as_pattern" => current.child_by_field_name("alias"),
            "delete_statement" => current.named_child(0),
            _ => None,
        };
        if visible
            && target.is_some_and(|target| names(content, target, name, remaining).unwrap_or(true))
        {
            binding = Binding::Local;
        }
        if matches!(current.kind(), "import_statement" | "import_from_statement") {
            let mut cursor = current.walk();
            for import in current.children_by_field_name("name", &mut cursor) {
                *remaining = remaining.checked_sub(1)?;
                let imported = import.child_by_field_name("name").unwrap_or(import);
                let local = import
                    .child_by_field_name("alias")
                    .map(|n| node_text(content, n))
                    .unwrap_or_else(|| {
                        node_text(content, imported)
                            .split('.')
                            .next()
                            .unwrap_or_default()
                            .to_owned()
                    });
                if visible && local == name {
                    binding = Binding::Local;
                }
            }
            continue;
        }
        if matches!(current.kind(), "parameters" | "lambda_parameters") {
            let mut cursor = current.walk();
            for parameter in current.named_children(&mut cursor) {
                *remaining = remaining.checked_sub(1)?;
                let target = match parameter.kind() {
                    "identifier" => Some(parameter),
                    "default_parameter" | "typed_default_parameter" => {
                        parameter.child_by_field_name("name")
                    }
                    "typed_parameter" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                        parameter.named_child(0)
                    }
                    _ => None,
                };
                if target
                    .is_some_and(|target| names(content, target, name, remaining).unwrap_or(true))
                {
                    binding = Binding::Local;
                }
            }
            continue;
        }
        if matches!(
            current.kind(),
            "function_definition" | "class_definition" | "lambda"
        ) {
            continue;
        }
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            *remaining = remaining.checked_sub(1)?;
            stack.push(child);
        }
    }
    (*remaining > 0).then_some(binding)
}

fn names(content: &str, node: Node<'_>, name: &str, remaining: &mut usize) -> Option<bool> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        *remaining = remaining.checked_sub(1)?;
        if current.kind() == "identifier" && node_text(content, current) == name {
            return Some(true);
        }
        if matches!(current.kind(), "attribute" | "subscript") {
            continue;
        }
        let mut cursor = current.walk();
        for child in current.named_children(&mut cursor) {
            *remaining = remaining.checked_sub(1)?;
            stack.push(child);
        }
    }
    Some(false)
}

#[cfg(test)]
#[path = "mutation_scopes_tests.rs"]
mod tests;
