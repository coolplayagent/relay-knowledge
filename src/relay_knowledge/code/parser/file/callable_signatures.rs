//! Bounded C-family declarator facts; display signatures remain unchanged.

use std::collections::HashSet;

use tree_sitter::Node;

use super::{FileParseContext, FileParseOutput};
use crate::domain::MAX_CALLABLE_SIGNATURE_KEY_BYTES;

const MAX_SIGNATURE_NODES: usize = 1024;
const MAX_FILE_SIGNATURE_NODES: usize = 65_536;
const PREFIX: &str = "c-family-callable-v1|";

pub(super) fn project(
    context: &FileParseContext<'_>,
    root: Node<'_>,
    output: &mut FileParseOutput,
) {
    if !matches!(context.language_id, "c" | "cpp") {
        return;
    }
    let mut remaining = MAX_FILE_SIGNATURE_NODES;
    let Some(macros) = macro_names(context.content, root, &mut remaining) else {
        return;
    };
    for symbol in &mut output.symbols {
        if remaining == 0 {
            break;
        }
        if !matches!(
            symbol.kind.as_str(),
            "function" | "function_declaration" | "method" | "constructor"
        ) {
            continue;
        }
        let Some(node) = root.descendant_for_byte_range(
            symbol.byte_range.start as usize,
            symbol.byte_range.end as usize,
        ) else {
            continue;
        };
        let mut budget = Budget {
            remaining: MAX_SIGNATURE_NODES,
            file: &mut remaining,
        };
        symbol.callable_signature_key =
            declarator(node, &symbol.name, context.content, &mut budget)
                .filter(|node| {
                    context.language_id != "c"
                        || node
                            .child_by_field_name("parameters")
                            .is_some_and(|parameters| parameters.named_child_count() != 0)
                })
                .and_then(|node| signature_key(context.content, node, &macros, &mut budget));
    }
}

struct Budget<'a> {
    remaining: usize,
    file: &'a mut usize,
}

impl Budget<'_> {
    fn spend(&mut self) -> Option<()> {
        self.remaining = self.remaining.checked_sub(1)?;
        *self.file = self.file.checked_sub(1)?;
        Some(())
    }
}

fn declarator<'tree>(
    mut node: Node<'tree>,
    name: &str,
    content: &str,
    budget: &mut Budget<'_>,
) -> Option<Node<'tree>> {
    let mut ancestor = Some(node);
    while let Some(current) = ancestor {
        budget.spend()?;
        if current.kind() == "template_declaration" {
            return None;
        }
        ancestor = current.parent();
    }
    while node.kind() != "function_declarator" {
        budget.spend()?;
        node = node.child_by_field_name("declarator")?;
    }
    let declared_name = binding_name(node.child_by_field_name("declarator")?, budget)?;
    let expected_name = name.rsplit([':', '.']).next()?;
    (declared_name.utf8_text(content.as_bytes()).ok()? == expected_name).then_some(node)
}

// Follow only the AST declarator ownership chain. Type identifiers, array bounds,
// and nested parameter types are never mistaken for the parameter's bound name.
fn binding_name<'tree>(mut node: Node<'tree>, budget: &mut Budget<'_>) -> Option<Node<'tree>> {
    loop {
        budget.spend()?;
        if matches!(node.kind(), "identifier" | "field_identifier") {
            return Some(node);
        }
        node = if matches!(
            node.kind(),
            "reference_declarator" | "parenthesized_declarator"
        ) {
            (node.named_child_count() == 1)
                .then(|| node.named_child(0))
                .flatten()?
        } else {
            node.child_by_field_name("declarator")
                .or_else(|| node.child_by_field_name("name"))?
        };
    }
}

fn macro_names<'a>(
    content: &'a str,
    root: Node<'_>,
    remaining: &mut usize,
) -> Option<HashSet<&'a str>> {
    // Source-local preprocessing can even redefine primitive keywords. Refuse
    // keys when any relevant token is a macro name, regardless of declaration
    // order or undef, instead of interpreting raw tokens as compiler types.
    let mut macro_nodes = vec![root];
    let mut macros = HashSet::new();
    while let Some(node) = macro_nodes.pop() {
        *remaining = remaining.checked_sub(1)?;
        if matches!(node.kind(), "preproc_def" | "preproc_function_def") {
            let name = node
                .child_by_field_name("name")?
                .utf8_text(content.as_bytes())
                .ok()?;
            macros.insert(name);
        }
        if macro_nodes.len().checked_add(node.named_child_count())? > *remaining {
            return None;
        }
        for index in 0..node.named_child_count() {
            macro_nodes.push(node.named_child(u32::try_from(index).ok()?)?);
        }
    }
    Some(macros)
}

fn signature_key(
    content: &str,
    function: Node<'_>,
    macros: &HashSet<&str>,
    budget: &mut Budget<'_>,
) -> Option<String> {
    if function.has_error()
        || function.end_byte().checked_sub(function.start_byte())?
            > MAX_CALLABLE_SIGNATURE_KEY_BYTES
    {
        return None;
    }
    if function
        .utf8_text(content.as_bytes())
        .ok()?
        .split(|value: char| !value.is_alphanumeric() && value != '_')
        .any(|token| macros.contains(token))
    {
        return None;
    }
    function.child_by_field_name("parameters")?;
    let mut member_cv_count = 0;
    for index in 0..function.named_child_count() {
        budget.spend()?;
        if function.named_child(u32::try_from(index).ok()?)?.kind() == "type_qualifier" {
            member_cv_count += 1;
        }
    }
    if member_cv_count > 1 {
        return None;
    }
    let mut ignored = HashSet::new();
    ignored.insert(function.child_by_field_name("declarator")?.id());
    let parameters = function.child_by_field_name("parameters")?;
    if parameters.named_child_count() == 1 {
        let parameter = parameters.named_child(0)?;
        if parameter.kind() == "parameter_declaration"
            && parameter.child_by_field_name("declarator").is_none()
            && parameter
                .child_by_field_name("type")
                .and_then(|kind| kind.utf8_text(content.as_bytes()).ok())
                == Some("void")
        {
            ignored.insert(parameter.id());
        }
    }
    let mut stack = vec![function];
    let mut result = PREFIX.to_owned();
    while let Some(node) = stack.pop() {
        budget.spend()?;
        if ignored.contains(&node.id()) || node.kind() == "comment" {
            continue;
        }
        if node.is_error()
            || node.is_missing()
            || matches!(
                node.kind(),
                "requires_clause"
                    | "trailing_return_type"
                    | "type_identifier"
                    | "qualified_identifier"
                    | "decltype"
                    | "placeholder_type_specifier"
                    | "sized_type_specifier"
                    | "array_declarator"
                    | "abstract_array_declarator"
                    | "attribute_specifier"
                    | "attribute_declaration"
                    | "struct_specifier"
                    | "class_specifier"
                    | "union_specifier"
                    | "enum_specifier"
                    | "noexcept"
                    | "throw_specifier"
                    | "virtual_specifier"
            )
        {
            return None;
        }
        if matches!(
            node.kind(),
            "parameter_declaration"
                | "optional_parameter_declaration"
                | "variadic_parameter_declaration"
        ) {
            // Without compiler type lookup, typedefs/macros and array adjustment
            // cannot prove identity. Primitive value cv is not part of the
            // callable type; pointee and member cv remain significant.
            let scalar = node
                .child_by_field_name("declarator")
                .is_none_or(|value| matches!(value.kind(), "identifier" | "field_identifier"));
            let mut qualifiers = 0;
            for index in 0..node.named_child_count() {
                budget.spend()?;
                if node.named_child(u32::try_from(index).ok()?)?.kind() == "type_qualifier" {
                    qualifiers += 1;
                }
            }
            if !scalar && qualifiers > 1 {
                return None;
            }
            if scalar {
                for index in 0..node.named_child_count() {
                    let child = node.named_child(u32::try_from(index).ok()?)?;
                    if child.kind() == "type_qualifier" {
                        ignored.insert(child.id());
                    }
                }
            }
            if let Some(declarator) = node.child_by_field_name("declarator") {
                if matches!(
                    declarator.kind(),
                    "function_declarator" | "abstract_function_declarator"
                ) && !declarator
                    .child_by_field_name("declarator")
                    .is_some_and(|inner| {
                        matches!(
                            inner.kind(),
                            "parenthesized_declarator" | "abstract_parenthesized_declarator"
                        )
                    })
                {
                    // Function parameters adjust to pointers. Only explicit
                    // pointer declarators have a normalized shape here.
                    return None;
                }
                if let Some(name) = binding_name(declarator, budget) {
                    ignored.insert(name.id());
                } else if !declarator.kind().starts_with("abstract_")
                    || budget.remaining == 0
                    || *budget.file == 0
                {
                    return None;
                }
            }
            if let Some(default) = node.child_by_field_name("default_value") {
                ignored.insert(default.id());
            }
        }
        if matches!(
            node.kind(),
            "parenthesized_declarator" | "abstract_parenthesized_declarator"
        ) && !(node.parent().is_some_and(|parent| {
            matches!(
                parent.kind(),
                "function_declarator" | "abstract_function_declarator"
            )
        }) && node.named_child_count() == 1
            && node.named_child(0).is_some_and(|child| {
                matches!(
                    child.kind(),
                    "pointer_declarator" | "abstract_pointer_declarator"
                )
            }))
        {
            return None;
        }
        if node.kind() == "type_qualifier"
            && node.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "pointer_declarator" | "abstract_pointer_declarator"
                )
            })
        {
            return None;
        }
        if node.kind() == "type_qualifier"
            && node
                .parent()
                .and_then(|parent| parent.child_by_field_name("type"))
                .is_some_and(|kind| kind.start_byte() < node.start_byte())
        {
            // Accept one canonical spelling of pointee cv; other legal orders
            // remain unknown rather than claiming unequal callable types.
            return None;
        }
        if node.kind() == "="
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "optional_parameter_declaration")
        {
            continue;
        }
        if node.child_count() == 0 {
            let token = node.utf8_text(content.as_bytes()).ok()?;
            if token.len() > MAX_CALLABLE_SIGNATURE_KEY_BYTES.saturating_sub(result.len()) {
                return None;
            }
            let encoded = format!("{}:{token}|", token.len());
            if result.len().checked_add(encoded.len())? > MAX_CALLABLE_SIGNATURE_KEY_BYTES {
                return None;
            }
            result.push_str(&encoded);
        } else {
            if stack.len().checked_add(node.child_count())? > budget.remaining.min(*budget.file) {
                return None;
            }
            for index in (0..node.child_count()).rev() {
                stack.push(node.child(u32::try_from(index).ok()?)?);
            }
        }
    }
    Some(result)
}

#[cfg(test)]
#[path = "callable_signatures_tests.rs"]
mod tests;
