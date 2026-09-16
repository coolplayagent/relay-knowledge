//! Retains receiver syntax so a matching method name cannot prove its target.
use super::super::nodes::static_declaration;
use std::collections::BTreeMap;
use tree_sitter::Node;

use crate::{
    code::CodeIndexError,
    domain::{CodeTypeOwner, RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord},
};

const MAX_WORK: usize = 1_000_000;
const MAX_HINT_BYTES: usize = 256;

pub(in crate::code::parser) fn bind_call_receivers(
    root: Node<'_>,
    content: &str,
    symbols: &[RepositoryCodeSymbolRecord],
    references: &mut [RepositoryCodeReferenceRecord],
) -> Result<(), CodeIndexError> {
    let cgo = super::cgo::unshadowed_package(root, content)?;
    let mut by_start = BTreeMap::<usize, Vec<usize>>::new();
    for (index, reference) in references
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == "call")
    {
        by_start
            .entry(reference.byte_range.start as usize)
            .or_default()
            .push(index);
    }
    let mut owners = BTreeMap::new();
    let mut targets = BTreeMap::<(&str, &str, bool), Vec<&RepositoryCodeSymbolRecord>>::new();
    for symbol in symbols {
        let is_static = root
            .named_descendant_for_byte_range(
                symbol.byte_range.start as usize,
                symbol.byte_range.end as usize,
            )
            .is_some_and(static_declaration);
        owners.insert(
            (
                symbol.byte_range.start as usize,
                symbol.byte_range.end as usize,
            ),
            symbol.type_owner.as_ref().map(|owner| (owner, is_static)),
        );
        if let Some(owner) = &symbol.type_owner
            && owner.relation == "direct_member"
            && crate::domain::code_call_targets::callable_target_symbol_kind(&symbol.kind)
        {
            targets
                .entry((&owner.identity, &symbol.name, is_static))
                .or_default()
                .push(symbol);
        }
    }
    let java = symbols.iter().any(|symbol| symbol.language_id == "java");
    let java_types = if java {
        super::java_receivers::JavaTypes::extract(root, content)?
    } else {
        None
    };
    let mut cursor = root.walk();
    let mut work = symbols.len() + references.len();
    loop {
        work += 1;
        if work > MAX_WORK {
            return Err(CodeIndexError::InvalidInput(
                "call receiver work budget exceeded".into(),
            ));
        }
        let node = cursor.node();
        if let Some((receiver, selector, member)) = receiver_selector(node) {
            let owner = if receiver.kind() == "this"
                && (!java
                    || (java_types.is_some()
                        && super::java_receivers::this_dispatch_proven(
                            node,
                            &content[member.byte_range()],
                        ))) {
                lexical_owner(node, &owners, &mut work)
            } else {
                None
            };
            let receiver_text = if receiver.byte_range().len() <= MAX_HINT_BYTES {
                &content[receiver.byte_range()]
            } else {
                "<complex receiver>"
            };
            let member_text = if member.byte_range().len() <= MAX_HINT_BYTES {
                &content[member.byte_range()]
            } else {
                "<member>"
            };
            for start in [selector.start_byte(), member.start_byte()]
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
            {
                let Some(indices) = by_start.get(&start) else {
                    continue;
                };
                for &index in indices {
                    work += 1;
                    if work > MAX_WORK {
                        return Err(CodeIndexError::InvalidInput(
                            "call receiver work budget exceeded".into(),
                        ));
                    }
                    let reference = &mut references[index];
                    if reference.byte_range.end as usize > selector.end_byte()
                        || (reference.name != content[member.byte_range()]
                            && reference.name != content[selector.byte_range()])
                    {
                        continue;
                    }
                    reference.target_hint = Some(format!("{receiver_text}.{member_text}"));
                    if cgo && receiver.kind() == "identifier" && receiver_text == "C" {
                        reference.name = format!("C.{member_text}");
                        reference.confidence_tier = "ambiguous".into();
                        continue;
                    }
                    reference.target_symbol_snapshot_id = None;
                    reference.resolution_state = "unresolved".into();
                    reference.confidence_basis_points = 2_500;
                    reference.confidence_tier = "extracted".into();
                    if let Some(qualified) = java_types.as_ref().and_then(|types| {
                        types.qualified_call(receiver, receiver_text, member_text)
                    }) {
                        reference.name = qualified;
                        reference.confidence_tier = "ambiguous".into();
                        continue;
                    }
                    let candidates = owner
                        .into_iter()
                        .flat_map(|(owner, is_static)| {
                            [false, true]
                                .into_iter()
                                .filter(move |kind| java || *kind == is_static)
                                .flat_map(|kind| {
                                    targets
                                        .get(&(
                                            owner.identity.as_str(),
                                            reference.name.as_str(),
                                            kind,
                                        ))
                                        .into_iter()
                                        .flatten()
                                })
                        })
                        .take(2)
                        .collect::<Vec<_>>();
                    if let [target] = candidates.as_slice() {
                        reference.target_symbol_snapshot_id =
                            Some(target.symbol_snapshot_id.clone());
                        reference.resolution_state = "resolved".into();
                        reference.confidence_basis_points = 10_000;
                        reference.confidence_tier = "exact".into();
                    }
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return Ok(());
            }
        }
    }
}

fn lexical_owner<'a>(
    mut node: Node<'_>,
    owners: &BTreeMap<(usize, usize), Option<(&'a CodeTypeOwner, bool)>>,
    work: &mut usize,
) -> Option<(&'a CodeTypeOwner, bool)> {
    let mut static_context = false;
    let origin = node.start_byte();
    let mut initializer = false;
    for _ in 0..128 {
        *work += 1;
        if *work > MAX_WORK {
            return None;
        }
        node = node.parent()?;
        if matches!(
            node.kind(),
            "class_static_block" | "field_definition" | "public_field_definition"
        ) {
            let value = node
                .child_by_field_name("value")
                .or_else(|| node.child_by_field_name("body"));
            if value.is_none_or(|value| origin < value.start_byte() || origin >= value.end_byte()) {
                return None;
            }
            initializer = true;
            static_context |= node.kind() == "class_static_block" || static_declaration(node);
        }
        if let Some(owner) = owners.get(&(node.start_byte(), node.end_byte())) {
            if owner.is_some_and(|(owner, _)| owner.relation == "declaration") && !initializer {
                return None;
            }
            if let Some(body) = node.child_by_field_name("body")
                && (origin < body.start_byte() || origin >= body.end_byte())
            {
                return None;
            }
            return owner
                .filter(|(o, _)| o.resolution_state.as_deref() == Some("resolved"))
                .map(|(owner, is_static)| (owner, is_static || static_context));
        }
    }
    None
}

fn receiver_selector(node: Node<'_>) -> Option<(Node<'_>, Node<'_>, Node<'_>)> {
    if matches!(
        node.kind(),
        "method_invocation" | "member_call_expression" | "call"
    ) {
        let receiver = node
            .child_by_field_name("object")
            .or_else(|| node.child_by_field_name("receiver"));
        if let Some(receiver) = receiver {
            let member = node
                .child_by_field_name("name")
                .or_else(|| node.child_by_field_name("method"))?;
            return Some((receiver, member, member));
        }
    }
    if !matches!(
        node.kind(),
        "call" | "call_expression" | "invocation_expression"
    ) {
        return None;
    }
    let mut function = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("expression"))?;
    for _ in 0..16 {
        if function.kind() != "parenthesized_expression" || function.named_child_count() != 1 {
            break;
        }
        function = function.named_child(0)?;
    }
    if !matches!(
        function.kind(),
        "member_expression"
            | "attribute"
            | "selector_expression"
            | "field_expression"
            | "member_access_expression"
    ) {
        return None;
    }
    let receiver = function
        .child_by_field_name("object")
        .or_else(|| function.child_by_field_name("operand"))
        .or_else(|| function.child_by_field_name("value"))
        .or_else(|| function.child_by_field_name("expression"))
        .or_else(|| function.child_by_field_name("argument"))?;
    let member = function
        .child_by_field_name("property")
        .or_else(|| function.child_by_field_name("attribute"))
        .or_else(|| function.child_by_field_name("field"))
        .or_else(|| function.child_by_field_name("name"))?;
    Some((receiver, function, member))
}
