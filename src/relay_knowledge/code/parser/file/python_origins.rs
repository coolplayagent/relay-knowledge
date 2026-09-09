//! Reconcile Python declaration evidence against this authorized source inventory.
use super::{FileParseContext, FileParseOutput};
use crate::code::parser::languages::python::is_overload_declaration_with_origins;
use std::collections::BTreeMap;
use tree_sitter::Node;

pub(super) fn reconcile(
    context: &FileParseContext<'_>,
    root: Node<'_>,
    output: &mut FileParseOutput,
) {
    if context.language_id != "python" {
        return;
    }
    let origins = context.build.python_module_origins;
    if origins.permits_standard_module("typing")
        && origins.permits_standard_module("typing_extensions")
    {
        return;
    }
    // Scope evidence only removes permission from the isolated proof. Ordinary
    // functions cannot gain a declaration classification from narrower origins.
    let mut declarations = output
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind == "function_declaration")
        .filter_map(|(index, symbol)| {
            usize::try_from(symbol.byte_range.start)
                .ok()
                .map(|start| (start, index))
        })
        .collect::<BTreeMap<_, _>>();
    if declarations.is_empty() {
        return;
    }
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.kind() == "function_definition" {
            if let Some(index) = declarations.remove(&node.start_byte()) {
                if !is_overload_declaration_with_origins(context.content, node, origins) {
                    output.symbols[index].kind = "function".to_owned();
                }
                if declarations.is_empty() {
                    return;
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "python_origins_tests.rs"]
mod tests;
