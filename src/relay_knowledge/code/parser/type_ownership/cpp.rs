//! Reconciles ownership with recovered C++ declaration facts on the same AST.
use super::*;

pub(in crate::code::parser) fn normalize_cpp_symbols(
    symbols: &mut Vec<RepositoryCodeSymbolRecord>,
) {
    let mut ends = BTreeMap::<u32, (u32, String)>::new();
    for symbol in symbols.iter().filter(|symbol| symbol.kind == "class") {
        ends.entry(symbol.byte_range.start)
            .and_modify(|(end, name)| {
                if symbol.byte_range.end > *end {
                    *end = symbol.byte_range.end;
                    name.clone_from(&symbol.name);
                }
            })
            .or_insert((symbol.byte_range.end, symbol.name.clone()));
    }
    // A macro-decorated class can produce a short "class EXPORT" capture and
    // a recovered full declaration. Only the full declaration defines a type.
    symbols.retain(|symbol| {
        symbol.kind != "class"
            || ends
                .get(&symbol.byte_range.start)
                .is_some_and(|(end, name)| *end == symbol.byte_range.end || *name == symbol.name)
    });
}

pub(super) fn indexed_types(
    root: Node<'_>,
    source: &str,
    symbols: &[RepositoryCodeSymbolRecord],
) -> BTreeMap<(usize, usize), String> {
    symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind.as_str(), "class" | "struct" | "enum" | "type"))
        .filter(|symbol| {
            root.named_descendant_for_byte_range(
                symbol.byte_range.start as usize,
                symbol.byte_range.end as usize,
            )
            .is_some_and(|node| {
                matches!(node.kind(), "function_definition" | "ERROR" | "declaration")
                    && !node.child_by_field_name("type").is_some_and(|ty| {
                        syntax::is_type("cpp", ty.kind())
                            && ty.child_by_field_name("body").is_some()
                    })
                    && node.start_byte() == symbol.byte_range.start as usize
                    && node.end_byte() == symbol.byte_range.end as usize
            })
        })
        .filter_map(|symbol| {
            let node = root.named_descendant_for_byte_range(
                symbol.byte_range.start as usize,
                symbol.byte_range.end as usize,
            )?;
            let name = recovered_name(node, source, &symbol.name)?;
            Some(((node.start_byte(), node.end_byte()), name))
        })
        .collect()
}

fn recovered_name(node: Node<'_>, source: &str, expected: &str) -> Option<String> {
    let mut name = node.child_by_field_name("declarator").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor).take(32).find(|child| {
            let leaf = child.child_by_field_name("name").unwrap_or(*child);
            source.get(leaf.byte_range()) == Some(expected)
        })
    })?;
    for _ in 0..16 {
        if matches!(
            name.kind(),
            "identifier" | "type_identifier" | "template_type" | "template_function"
        ) {
            let leaf = name.child_by_field_name("name").unwrap_or(name);
            if source.get(leaf.byte_range()) != Some(expected) {
                return None;
            }
            return templates::name(name, node, source);
        }
        name = name.child_by_field_name("declarator")?;
    }
    None
}

pub(super) fn namespace(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() != "function_definition" {
        return None;
    }
    let prefix = node.child_by_field_name("type")?;
    let keyword = node.child_by_field_name("declarator")?;
    let body = node.child_by_field_name("body")?;
    if prefix.kind() != "type_identifier"
        || keyword.kind() != "identifier"
        || keyword.utf8_text(source.as_bytes()).ok()? != "namespace"
        || body.kind() != "compound_statement"
    {
        return None;
    }
    let macro_name = source.get(prefix.byte_range())?;
    if macro_name.len() > 256
        || !macro_name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return None;
    }
    let raw_name = source.get(keyword.end_byte()..body.start_byte())?;
    if raw_name.len() > 1024 {
        return None;
    }
    let name = raw_name.trim();
    if name.len() > 1024
        || name.split("::").count() > 16
        || !name.split("::").all(|part| {
            let mut bytes = part.bytes();
            bytes
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
    {
        return None;
    }
    // Preserve the opaque wrapper in identity; do not claim macro expansion or
    // merge a differently wrapped namespace with this declaration.
    Some(format!("macro@{macro_name}.{}", name.replace("::", ".")))
}
