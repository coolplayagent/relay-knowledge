use tree_sitter::Node;
use unicode_ident::{is_xid_continue, is_xid_start};

use super::super::nodes::{SyntaxRange, node_text, syntax_range};

pub(in crate::code::parser) fn enum_member_definition(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Option<(String, &'static str, SyntaxRange)> {
    if !enum_member_node(language_id, node.kind()) {
        return None;
    }
    let name = enum_member_name_node(node).map(|name| node_text(content, name))?;
    if !enum_member_name(language_id, &name) {
        return None;
    }

    Some((name, "enum_member", syntax_range(node)))
}

fn enum_member_node(language_id: &str, node_kind: &str) -> bool {
    match language_id {
        "c" | "cpp" => node_kind == "enumerator",
        "rust" => node_kind == "enum_variant",
        _ => false,
    }
}

fn enum_member_name_node(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).find(|child| {
            matches!(
                child.kind(),
                "identifier" | "property_identifier" | "raw_identifier" | "type_identifier"
            )
        })
    })
}

fn enum_member_name(language_id: &str, name: &str) -> bool {
    if language_id == "rust" {
        return name
            .strip_prefix("r#")
            .map_or_else(|| rust_enum_member_name(name), rust_enum_member_name);
    }

    c_like_enum_member_name(name)
}

fn rust_enum_member_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || is_xid_start(character))
        && characters.all(|character| character == '_' || is_xid_continue(character))
}

fn c_like_enum_member_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}
