//! Bounded proof that a direct invocation reaches this decorator synchronously.
use tree_sitter::Node;

pub(super) fn reaches(
    content: &str,
    function: Node<'_>,
    decorated: Node<'_>,
    remaining: &mut usize,
) -> bool {
    let Some(body) = function.child_by_field_name("body") else {
        return false;
    };
    if decorated.parent() != Some(body) {
        return false;
    }
    let mut cursor = function.walk();
    for child in function.children(&mut cursor) {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        if child.kind() == "async" {
            return false;
        }
    }
    // A yield anywhere in this function makes even its earlier statements
    // deferred. Nested function bodies do not make the enclosing function a generator.
    let deferred = super::expressions::future_annotations(content, function, remaining);
    let mut stack = vec![body];
    while let Some(node) = stack.pop() {
        let Some(left) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = left;
        if node.kind() == "yield" {
            return false;
        }
        if !super::expressions::eager_children(node, &mut stack, remaining, deferred) {
            return false;
        }
    }
    // The first call must not bypass this decorator and leave it for a later
    // invocation after a provider write. Reject control flow and eager calls.
    let mut previous = decorated.prev_named_sibling();
    while let Some(statement) = previous {
        let mut stack = vec![statement];
        while let Some(node) = stack.pop() {
            let Some(left) = remaining.checked_sub(1) else {
                return false;
            };
            *remaining = left;
            if matches!(
                node.kind(),
                "return_statement"
                    | "raise_statement"
                    | "if_statement"
                    | "for_statement"
                    | "while_statement"
                    | "try_statement"
                    | "with_statement"
                    | "match_statement"
                    | "import_statement"
                    | "import_from_statement"
                    | "call"
                    | "await"
                    | "yield"
            ) {
                return false;
            }
            if !super::expressions::eager_children(node, &mut stack, remaining, deferred) {
                return false;
            }
        }
        previous = statement.prev_named_sibling();
    }
    true
}

#[cfg(test)]
#[path = "direct_execution_tests.rs"]
mod tests;
