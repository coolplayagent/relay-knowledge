//! File-wide conservative proof of Java type names, from the existing AST.
use crate::{code::CodeIndexError, domain::code_call_targets::java_name_path};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

pub(super) struct JavaTypes {
    package: String,
    imports: BTreeMap<String, String>,
    blocked: BTreeSet<String>,
    local: BTreeSet<String>,
}

impl JavaTypes {
    pub(super) fn extract(root: Node<'_>, source: &str) -> Result<Option<Self>, CodeIndexError> {
        if root.has_error() {
            return Ok(None);
        }
        let mut result = Self {
            package: String::new(),
            imports: BTreeMap::new(),
            blocked: BTreeSet::new(),
            local: BTreeSet::new(),
        };
        let mut cursor = root.walk();
        for _ in 0..1_000_000 {
            let node = cursor.node();
            let kind = node.kind();
            if matches!(
                kind,
                "superclass" | "super_interfaces" | "extends_interfaces"
            ) || (kind == "class_body"
                && node.parent().is_some_and(|p| {
                    matches!(p.kind(), "object_creation_expression" | "enum_constant")
                }))
            {
                return Ok(None);
            }
            if matches!(kind, "package_declaration" | "import_declaration") {
                let mut walk = node.walk();
                if node
                    .children(&mut walk)
                    .any(|child| child.kind() == "static")
                {
                    return Ok(None);
                }
                let mut walk = node.walk();
                let path = node
                    .named_children(&mut walk)
                    .find(|child| matches!(child.kind(), "identifier" | "scoped_identifier"));
                if let Some(path) = path {
                    let text = &source[path.byte_range()];
                    if !java_name_path(text) {
                        return Ok(None);
                    }
                    if kind == "package_declaration" {
                        result.package = text.into();
                    } else {
                        let mut walk = node.walk();
                        if !node
                            .children(&mut walk)
                            .any(|child| child.kind() == "asterisk" || child.kind() == "*")
                        {
                            let name = text.rsplit('.').next().unwrap_or(text).to_owned();
                            if result
                                .imports
                                .insert(name.clone(), text.into())
                                .is_some_and(|previous| previous != text)
                            {
                                result.blocked.insert(name);
                            }
                        }
                    }
                }
            }
            if matches!(kind, "identifier" | "type_identifier") {
                let parent = node.parent();
                let binding = cursor.field_name() == Some("name")
                    && parent.is_some_and(|p| {
                        matches!(
                            p.kind(),
                            "variable_declarator"
                                | "formal_parameter"
                                | "spread_parameter"
                                | "catch_formal_parameter"
                                | "resource"
                                | "enhanced_for_statement"
                                | "instanceof_expression"
                                | "class_declaration"
                                | "interface_declaration"
                                | "enum_declaration"
                                | "record_declaration"
                                | "annotation_type_declaration"
                                | "enum_constant"
                                | "type_parameter"
                        )
                    });
                let shorthand = parent.is_some_and(|p| {
                    matches!(p.kind(), "inferred_parameters" | "type_parameter")
                        || (kind == "identifier"
                            && matches!(p.kind(), "type_pattern" | "record_pattern_component"))
                        || (p.kind() == "lambda_expression"
                            && p.child_by_field_name("parameters") == Some(node))
                });
                if binding || shorthand {
                    let name = &source[node.byte_range()];
                    if name.len() > 256 {
                        return Ok(None);
                    }
                    if parent.is_some_and(|p| {
                        matches!(
                            p.kind(),
                            "class_declaration"
                                | "interface_declaration"
                                | "enum_declaration"
                                | "record_declaration"
                        ) && p.parent() == Some(root)
                    }) {
                        if !result.local.insert(name.into()) {
                            result.blocked.insert(name.into());
                        }
                    } else {
                        result.blocked.insert(name.into());
                    }
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return Ok(Some(result));
                }
            }
        }
        Err(CodeIndexError::InvalidInput(
            "Java receiver proof work budget exceeded".into(),
        ))
    }

    pub(super) fn qualified_call(
        &self,
        receiver: Node<'_>,
        receiver_text: &str,
        member: &str,
    ) -> Option<String> {
        if receiver.kind() != "identifier"
            || self.blocked.contains(receiver_text)
            || !java_name_path(receiver_text)
            || !java_name_path(member)
        {
            return None;
        }
        let ty = if self.local.contains(receiver_text) {
            if self.imports.contains_key(receiver_text) {
                return None;
            }
            None
        } else {
            self.imports.get(receiver_text)
        };
        Some(match ty {
            Some(ty) => format!("{ty}.{member}"),
            None if self.package.is_empty() => format!("{receiver_text}.{member}"),
            None => format!("{}.{receiver_text}.{member}", self.package),
        })
    }
}

/// Names inherited from Object and compiler-generated enum/record members need
/// overload evidence that this bounded receiver analysis does not provide.
pub(super) fn this_dispatch_proven(mut node: Node<'_>, member: &str) -> bool {
    if matches!(
        member,
        "equals"
            | "hashCode"
            | "toString"
            | "clone"
            | "finalize"
            | "getClass"
            | "notify"
            | "notifyAll"
            | "wait"
    ) {
        return false;
    }
    for _ in 0..128 {
        match node.kind() {
            "class_declaration" | "interface_declaration" => return true,
            "enum_declaration" | "record_declaration" => return false,
            _ => {}
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
    false
}
