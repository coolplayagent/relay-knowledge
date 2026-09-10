//! Bounded compilation-unit evidence shared by Java parsing and configuration reads.
use std::collections::BTreeSet;
use tree_sitter::Node;

use crate::domain::JavaNamespaceEvidence;

pub(super) struct JavaFileNamespace {
    pub(super) evidence: JavaNamespaceEvidence,
    pub(super) explicit_platform_types: BTreeSet<String>,
}

pub(super) fn collect(root: Node<'_>, source: &str) -> JavaFileNamespace {
    let mut result = JavaFileNamespace {
        evidence: JavaNamespaceEvidence {
            source_set: Default::default(),
            package: String::new(),
            top_level_types: Vec::new(),
            complete: !root.has_error(),
        },
        explicit_platform_types: BTreeSet::new(),
    };
    let mut remaining = 1024usize;
    let mut bytes = JavaNamespaceEvidence::MAX_PROJECTED_NAME_BYTES;
    let mut package_seen = false;
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        let Some(next) = remaining.checked_sub(1) else {
            result.evidence.complete = false;
            break;
        };
        remaining = next;
        match child.kind() {
            "package_declaration" => {
                if package_seen {
                    result.evidence.complete = false;
                }
                package_seen = true;
                match declared_name(child, source, &mut remaining, &mut bytes) {
                    Some(name) => result.evidence.package = name,
                    None => result.evidence.complete = false,
                }
            }
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "annotation_type_declaration" => {
                match child
                    .child_by_field_name("name")
                    .and_then(|node| spelling(node, source, &mut remaining, &mut bytes))
                {
                    Some(name) => result.evidence.top_level_types.push(name),
                    None => result.evidence.complete = false,
                }
            }
            "import_declaration" => {
                let Some((name, is_static_or_glob)) = declaration_name(child, &mut remaining)
                else {
                    result.evidence.complete = false;
                    break;
                };
                if is_static_or_glob {
                    continue;
                }
                if let Some(name) =
                    name.and_then(|node| spelling(node, source, &mut remaining, &mut bytes))
                {
                    if let Some(name) = name.strip_prefix("java.lang.") {
                        if matches!(name, "System" | "Boolean") {
                            result.explicit_platform_types.insert(name.to_owned());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    result.evidence.top_level_types.sort();
    if result
        .evidence
        .top_level_types
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        result.evidence.complete = false;
    }
    result.evidence.top_level_types.dedup();
    if result.evidence.projected_name_bytes().is_none() {
        result.evidence.complete = false;
        result.evidence.top_level_types.clear();
    }
    result
}

/// Resolve a declaration's qualified identifier using the caller's shared work budget.
pub(in crate::code) fn declared_name(
    declaration: Node<'_>,
    source: &str,
    nodes: &mut usize,
    bytes: &mut usize,
) -> Option<String> {
    let (name, _) = declaration_name(declaration, nodes)?;
    spelling(name?, source, nodes, bytes)
}

fn spelling(node: Node<'_>, source: &str, nodes: &mut usize, bytes: &mut usize) -> Option<String> {
    let mut result = String::new();
    let mut cursor = node.walk();
    loop {
        *nodes = nodes.checked_sub(1)?;
        let current = cursor.node();
        match current.kind() {
            "identifier" => {
                let text = source.get(current.byte_range())?;
                if text.is_empty()
                    || !text
                        .chars()
                        .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '$'))
                {
                    return None;
                }
                let separator = usize::from(!result.is_empty());
                *bytes = bytes.checked_sub(text.len().checked_add(separator)?)?;
                if separator != 0 {
                    result.push('.');
                }
                result.push_str(text);
            }
            "scoped_identifier" => {
                if cursor.goto_first_child() {
                    continue;
                }
                return None;
            }
            "." | "line_comment" | "block_comment" => {}
            _ => return None,
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return (!result.is_empty()).then_some(result);
            }
        }
    }
}

fn declaration_name<'a>(node: Node<'a>, remaining: &mut usize) -> Option<(Option<Node<'a>>, bool)> {
    let mut result = None;
    let mut special = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        *remaining = remaining.checked_sub(1)?;
        special |= matches!(child.kind(), "static" | "asterisk");
        if matches!(child.kind(), "identifier" | "scoped_identifier") {
            result = Some(child);
        }
    }
    Some((result, special))
}

#[cfg(test)]
#[path = "java_namespace_tests.rs"]
mod tests;
