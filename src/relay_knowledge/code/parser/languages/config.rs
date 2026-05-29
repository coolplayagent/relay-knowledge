use tree_sitter::Node;

use crate::code::parser::nodes::{SyntaxRange, node_text, syntax_range};

pub(in crate::code::parser) fn definition_kind(
    language_id: &str,
    node_kind: &str,
) -> Option<&'static str> {
    match language_id {
        "json" if node_kind == "pair" => Some("config"),
        "yaml" if matches!(node_kind, "block_mapping_pair" | "flow_pair") => Some("config"),
        "properties" if node_kind == "property" => Some("config"),
        _ => None,
    }
}

pub(in crate::code::parser) fn manual_definition_candidate(
    language_id: &str,
    node_kind: &str,
) -> bool {
    definition_kind(language_id, node_kind).is_some()
}

pub(in crate::code::parser) fn manual_definitions(
    content: &str,
    language_id: &str,
    node: Node<'_>,
) -> Vec<(String, &'static str, SyntaxRange)> {
    let Some(kind) = definition_kind(language_id, node.kind()) else {
        return Vec::new();
    };
    let Some(name) = (match language_id {
        "json" => json_pair_path(content, node),
        "yaml" => yaml_pair_path(content, node),
        "properties" => properties_key(content, node),
        _ => None,
    }) else {
        return Vec::new();
    };
    vec![(name, kind, syntax_range(node))]
}

fn json_pair_path(content: &str, node: Node<'_>) -> Option<String> {
    let key = json_pair_key(content, node)?;
    let mut parts = ancestor_pair_keys(content, node, "pair", json_pair_key);
    parts.push(key);
    Some(parts.join("."))
}

fn yaml_pair_path(content: &str, node: Node<'_>) -> Option<String> {
    let key = yaml_pair_key(content, node)?;
    let mut parts = ancestor_pair_keys(content, node, "block_mapping_pair", yaml_pair_key);
    parts.extend(ancestor_pair_keys(
        content,
        node,
        "flow_pair",
        yaml_pair_key,
    ));
    parts.push(key);
    Some(parts.join("."))
}

fn ancestor_pair_keys(
    content: &str,
    node: Node<'_>,
    pair_kind: &str,
    key_fn: fn(&str, Node<'_>) -> Option<String>,
) -> Vec<String> {
    let mut keys = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == pair_kind
            && let Some(key) = key_fn(content, parent)
        {
            keys.push(key);
        }
        current = parent.parent();
    }
    keys.reverse();
    keys
}

fn json_pair_key(content: &str, node: Node<'_>) -> Option<String> {
    let key = node.child_by_field_name("key")?;
    unquote_config_key(&node_text(content, key))
}

fn yaml_pair_key(content: &str, node: Node<'_>) -> Option<String> {
    let key = node.child_by_field_name("key")?;
    let text = scalar_text(content, key);
    unquote_config_key(&text)
}

fn scalar_text(content: &str, node: Node<'_>) -> String {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if matches!(
            current.kind(),
            "string_scalar"
                | "plain_scalar"
                | "single_quote_scalar"
                | "double_quote_scalar"
                | "boolean_scalar"
                | "integer_scalar"
                | "float_scalar"
                | "null_scalar"
        ) {
            return node_text(content, current);
        }
        for index in (0..current.child_count()).rev() {
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            if let Some(child) = current.child(index) {
                stack.push(child);
            }
        }
    }
    node_text(content, node)
}

fn properties_key(content: &str, node: Node<'_>) -> Option<String> {
    (0..node.named_child_count()).find_map(|index| {
        let child = node.named_child(u32::try_from(index).ok()?)?;
        (child.kind() == "key").then(|| node_text(content, child))
    })
}

fn unquote_config_key(value: &str) -> Option<String> {
    let key = value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_owned();
    (!key.is_empty() && !key.contains('\n')).then_some(key)
}
