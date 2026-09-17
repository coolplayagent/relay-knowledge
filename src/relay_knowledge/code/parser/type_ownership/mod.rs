//! Explicit type ownership derived at indexing time from language syntax.

mod cpp;
mod imports;
mod syntax;
mod templates;
pub(super) use cpp::normalize_cpp_symbols;

use std::collections::BTreeMap;
use tree_sitter::Node;

use crate::{
    code::CodeIndexError,
    domain::{CodeTypeOwner, RepositoryCodeSymbolRecord},
};

const MAX_NODES: usize = 262_144;
const MAX_ANCESTORS: usize = 128;

pub(super) fn extract(
    root: Node<'_>,
    content: &str,
    path: &str,
    language: &str,
    symbols: &mut [RepositoryCodeSymbolRecord],
) -> Result<(), CodeIndexError> {
    if !syntax::supports_types(language) {
        return Ok(());
    }
    let cpp_types = if language == "cpp" {
        cpp::indexed_types(root, content, symbols)
    } else {
        BTreeMap::new()
    };
    let mut by_start = BTreeMap::<usize, Vec<usize>>::new();
    for (index, symbol) in symbols.iter().enumerate() {
        by_start
            .entry(symbol.byte_range.start as usize)
            .or_default()
            .push(index);
    }
    let module = syntax::module_identity(root, content, path, language)?;
    let mut cursor = root.walk();
    let mut visited = 0;
    'syntax: loop {
        visited += 1;
        if visited > MAX_NODES {
            return Err(incomplete("syntax node budget exceeded"));
        }
        let node = cursor.node();
        if syntax::is_type(language, node.kind())
            || cpp_types.contains_key(&(node.start_byte(), node.end_byte()))
            || syntax::is_callable(node.kind())
            || syntax::is_callable_field(node, language)
            || (language == "rust" && node.kind() == "mod_item")
        {
            let range = if language == "go" && node.kind() == "type_spec" {
                node.parent()
                    .filter(|p| p.kind() == "type_declaration" && p.named_child_count() == 1)
                    .unwrap_or(node)
            } else {
                node
            };
            let mut ranges = vec![range];
            if matches!(language, "javascript" | "jsx" | "typescript" | "tsx")
                && let Some(export) = node
                    .parent()
                    .filter(|parent| parent.kind() == "export_statement")
            {
                ranges.push(export);
            }
            for range in ranges {
                if let Some(indices) = by_start.get(&range.start_byte()) {
                    let ownership = if language == "rust" && node.kind() == "mod_item" {
                        imports::rust_module_declaration(node, content, path)
                    } else {
                        owner(node, content, &module, path, language, &cpp_types)?
                    };
                    for index in indices {
                        if symbols[*index].byte_range.end as usize == range.end_byte()
                            && (!syntax::is_type(language, node.kind())
                                || matches!(
                                    symbols[*index].kind.as_str(),
                                    "class"
                                        | "type"
                                        | "interface"
                                        | "struct"
                                        | "enum"
                                        | "trait"
                                        | "module"
                                ))
                        {
                            symbols[*index].type_owner.clone_from(&ownership);
                        }
                    }
                }
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'syntax;
            }
        }
    }
    let mut declarations = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for symbol in symbols.iter() {
        if let Some(owner) = &symbol.type_owner
            && owner.relation == "declaration"
            && let Some(lookup) = &owner.lookup_identity
        {
            declarations
                .entry(lookup.clone())
                .or_default()
                .insert(owner.identity.clone());
        }
    }
    let mut imported_paths = BTreeMap::new();
    for symbol in symbols.iter_mut() {
        if let Some(owner) = &mut symbol.type_owner
            && !matches!(owner.basis.as_deref(), Some("lexical" | "rust_module"))
        {
            if !imported_paths.contains_key(&owner.target_hint) {
                imported_paths.insert(
                    owner.target_hint.clone(),
                    imports::target_paths(root, content, path, language, &owner.target_hint)?,
                );
            }
            (owner.target_paths, owner.import_target) = imported_paths[&owner.target_hint].clone();
            if let Some(candidates) = owner
                .lookup_identity
                .as_ref()
                .and_then(|key| declarations.get(key))
            {
                if candidates.len() == 1 {
                    owner.identity = candidates.first().expect("one declaration").clone();
                    owner.resolution_state = Some("resolved".into());
                } else {
                    owner.resolution_state = Some("ambiguous".into());
                }
            }
        }
    }
    Ok(())
}

fn owner(
    node: Node<'_>,
    source: &str,
    module: &str,
    path: &str,
    language: &str,
    cpp_types: &BTreeMap<(usize, usize), String>,
) -> Result<Option<CodeTypeOwner>, CodeIndexError> {
    if matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "generator_function"
    ) {
        // Named callable fields have their own declaration symbol. An inline
        // expression is a local call owner, including field initializer callbacks.
        return Ok(None);
    }
    if language == "swift"
        && node
            .child_by_field_name("declaration_kind")
            .is_some_and(|kind| kind.kind() == "extension")
    {
        return Ok(None);
    }
    if language == "ruby"
        && node.kind() == "singleton_method"
        && node
            .child_by_field_name("object")
            .is_some_and(|receiver| receiver.utf8_text(source.as_bytes()).ok() != Some("self"))
    {
        return Ok(None);
    }
    let declaration = syntax::is_type(language, node.kind())
        || cpp_types.contains_key(&(node.start_byte(), node.end_byte()));
    let mut current = if declaration {
        Some(node)
    } else {
        node.parent()
    };
    let mut names = Vec::new();
    let mut anchor = None;
    let mut relation = if declaration {
        "declaration"
    } else {
        "direct_member"
    };
    let mut basis = "lexical";
    if !declaration {
        if let Some((name, kind)) = syntax::detached_owner(node, source, language) {
            names.push(name);
            relation = kind;
            basis = if language == "go" {
                "go_receiver"
            } else if language == "kotlin" {
                "kotlin_extension"
            } else {
                "cpp_qualified"
            };
        }
    }
    for _ in 0..MAX_ANCESTORS {
        let Some(ancestor) = current else {
            if names.is_empty() || (basis == "lexical" && anchor.is_none()) {
                return Ok(None);
            }
            names.reverse();
            let hint = names.join(".");
            if basis == "cpp_qualified" && !templates::arguments_proven(&hint) {
                basis = "cpp_unresolved_template";
            }
            let lookup = format!("{module}|{hint}");
            return Ok(Some(CodeTypeOwner {
                static_dispatch: (language == "java")
                    .then(|| super::nodes::java_static_dispatch(node)),
                identity: if basis == "lexical" {
                    format!("{lookup}@{path}:{}", anchor.unwrap_or(node.start_byte()))
                } else {
                    format!("unresolved|{path}|{}", node.start_byte())
                },
                relation: relation.to_owned(),
                target_hint: hint,
                lookup_identity: (basis != "cpp_unresolved_template").then_some(lookup),
                basis: Some(basis.to_owned()),
                resolution_state: Some(
                    if basis == "lexical" {
                        "resolved"
                    } else {
                        "unresolved"
                    }
                    .to_owned(),
                ),
                target_paths: Vec::new(),
                import_target: None,
                visibility: syntax::visibility(node, source, language),
            }));
        };
        if language == "ruby"
            && ancestor.kind() == "singleton_class"
            && ancestor
                .child_by_field_name("value")
                .is_none_or(|value| value.utf8_text(source.as_bytes()).ok() != Some("self"))
        {
            return Ok(None);
        }
        if let Some(name) = cpp_types.get(&(ancestor.start_byte(), ancestor.end_byte())) {
            anchor.get_or_insert(ancestor.start_byte());
            names.push(name.clone());
        } else if language == "cpp"
            && let Some(name) = cpp::namespace(ancestor, source)
        {
            names.push(name);
        } else if syntax::is_callable(ancestor.kind())
            || (language == "java"
                && ancestor.kind() == "class_body"
                && ancestor.parent().is_some_and(|parent| {
                    matches!(
                        parent.kind(),
                        "object_creation_expression" | "enum_constant"
                    )
                }))
        {
            // A local function is not a direct member; a local type has a lexical identity.
            if names.is_empty() {
                return Ok(None);
            }
            names.push(format!("local@{}", ancestor.start_byte()));
        } else if language == "scala" && ancestor.kind() == "extension_definition" {
            basis = "scala_extension";
            let Some(parameters) = ancestor.child_by_field_name("parameters") else {
                return Ok(None);
            };
            if parameters.named_child_count() != 1 {
                return Ok(None);
            }
            let Some(target) = parameters
                .named_child(0)
                .and_then(|n| n.child_by_field_name("type"))
                .and_then(|n| syntax::type_name(n, source))
            else {
                return Ok(None);
            };
            names.push(target);
        } else if language == "swift"
            && ancestor
                .child_by_field_name("declaration_kind")
                .is_some_and(|kind| kind.kind() == "extension")
        {
            basis = "swift_extension";
            let Some(name) = syntax::name(ancestor, source) else {
                return Ok(None);
            };
            names.push(name);
        } else if syntax::is_type(language, ancestor.kind())
            || syntax::is_namespace(ancestor.kind())
        {
            if syntax::is_type(language, ancestor.kind()) {
                anchor.get_or_insert(ancestor.start_byte());
            }
            let Some(name) = syntax::name(ancestor, source) else {
                return Ok(None);
            };
            names.push(name);
        } else if language == "rust" && ancestor.kind() == "impl_item" {
            basis = "rust_impl";
            let Some(target) = ancestor.child_by_field_name("type") else {
                return Ok(None);
            };
            let Some(name) = syntax::type_name(target, source) else {
                return Ok(None);
            };
            names.push(name);
            if ancestor.child_by_field_name("trait").is_some() {
                relation = "trait_member";
            }
        } else if language == "swift" && ancestor.kind() == "extension_declaration" {
            basis = "swift_extension";
            let Some(name) = syntax::name(ancestor, source) else {
                return Ok(None);
            };
            names.push(name);
        }
        current = ancestor.parent();
    }
    Err(incomplete("lexical ownership depth exceeded"))
}

fn incomplete(reason: &str) -> CodeIndexError {
    CodeIndexError::InvalidInput(format!("type ownership incomplete: {reason}"))
}

#[cfg(test)]
mod mod_tests;
