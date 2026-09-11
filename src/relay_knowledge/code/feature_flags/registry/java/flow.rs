//! Conservative configuration getter flow and explicit key declaration conventions.
use super::names;
use tree_sitter::Node;

pub(super) fn returning_method<'a>(
    mut node: Node<'a>,
    content: &str,
) -> Option<(Node<'a>, Vec<String>)> {
    let mut shadows = std::collections::BTreeSet::new();
    for _ in 0..32 {
        let parent = node.parent()?;
        match parent.kind() {
            "parenthesized_expression" | "cast_expression" => node = parent,
            "argument_list" if parent.named_child_count() == 1 => {
                let call = parent.parent()?;
                let owner = conversion(call, content)?;
                if let Some(shadow) = super::types::platform_shadow(call, owner, content) {
                    shadows.insert(shadow);
                }
                node = call;
            }
            "return_statement" => {
                let body = parent.parent().filter(|n| {
                    let mut cursor = n.walk();
                    n.kind() == "block"
                        && n.named_children(&mut cursor)
                            .filter(|child| {
                                !matches!(child.kind(), "line_comment" | "block_comment")
                            })
                            .take(2)
                            .count()
                            == 1
                })?;
                return body
                    .parent()
                    .filter(|method| {
                        method.kind() == "method_declaration"
                            && method
                                .child_by_field_name("parameters")
                                .is_some_and(|p| p.named_child_count() == 0)
                    })
                    .map(|method| (method, shadows.into_iter().collect()));
            }
            _ => return None,
        }
    }
    None
}
fn conversion<'a>(call: Node<'_>, content: &'a str) -> Option<&'a str> {
    if call.kind() != "method_invocation" {
        return None;
    }
    let name = call.child_by_field_name("name")?;
    let method = names::text(name, content);
    let owner = call
        .child_by_field_name("object")
        .map(|n| names::text(n, content))
        .or_else(|| names::static_owner(call, method, content));
    let owner = owner?;
    let simple = owner.strip_prefix("java.lang.").unwrap_or(owner);
    let supported = matches!(
        (simple, method),
        ("Boolean", "parseBoolean" | "valueOf")
            | ("Integer", "parseInt" | "valueOf")
            | ("Long", "parseLong" | "valueOf")
            | ("Double", "parseDouble" | "valueOf")
    );
    (supported
        && (owner.starts_with("java.lang.") || names::platform_visible(call, simple, content)))
    .then_some(owner)
}
pub(super) fn inside_getter(mut node: Node<'_>, content: &str) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == "method_declaration" {
            return parent.child_by_field_name("name").is_some_and(|n| {
                let name = names::text(n, content);
                name.starts_with("get") || name.starts_with("is")
            }) && parent
                .child_by_field_name("parameters")
                .is_some_and(|p| p.named_child_count() == 0);
        }
        if matches!(parent.kind(), "lambda_expression" | "class_declaration") {
            return false;
        }
        node = parent;
    }
    false
}
pub(super) fn key_declaration(mut node: Node<'_>, content: &str) -> bool {
    if node
        .child_by_field_name("name")
        .is_some_and(|n| names::text(n, content).ends_with("_KEY"))
    {
        return true;
    }
    while let Some(parent) = node.parent() {
        if matches!(parent.kind(), "class_declaration" | "interface_declaration") {
            // A documented declaration convention, independent of a particular repository/key.
            return parent
                .child_by_field_name("name")
                .is_some_and(|n| names::text(n, content).ends_with("Keys"));
        }
        node = parent;
    }
    false
}
