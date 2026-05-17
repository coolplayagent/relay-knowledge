use tree_sitter::Node;

use crate::domain::{CodeImportRecord, RepositoryCodeRange};

use super::{
    super::{CodeIndexError, SnapshotBuild, stable_id},
    nodes::{compact_whitespace, node_text, push_children_reverse, syntax_range},
};

pub(super) fn collect_imports(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    root: Node<'_>,
) -> Result<Vec<CodeImportRecord>, CodeIndexError> {
    let mut imports = Vec::new();
    let mut stack = Vec::with_capacity(root.child_count().saturating_add(1));
    stack.push(root);
    while let Some(node) = stack.pop() {
        if is_import_node(node) {
            push_import_records(
                build,
                path,
                file_id,
                language_id,
                content,
                node,
                &mut imports,
            )?;
        }
        push_children_reverse(node, &mut stack);
    }

    Ok(imports)
}

fn push_import_records(
    build: &SnapshotBuild,
    path: &str,
    file_id: &str,
    language_id: &str,
    content: &str,
    node: Node<'_>,
    imports: &mut Vec<CodeImportRecord>,
) -> Result<(), CodeIndexError> {
    let module_text = node_text(content, node);
    let modules = if language_id == "go" {
        go_import_specs(&module_text)
    } else {
        Vec::new()
    };
    let modules = if modules.is_empty() {
        vec![compact_whitespace(&module_text)]
    } else {
        modules
    };
    let range = syntax_range(node);
    for module in modules {
        imports.push(CodeImportRecord {
            repository_id: build.repository_id.clone(),
            source_scope: build.source_scope.clone(),
            import_id: stable_id(
                "import",
                [
                    &build.repository_id,
                    &build.source_scope,
                    path,
                    &module,
                    &range.line_start.to_string(),
                    &range.line_end.to_string(),
                ],
            ),
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            module,
            target_hint: None,
            resolution_state: "unresolved".to_owned(),
            confidence_basis_points: 10_000,
            confidence_tier: "extracted".to_owned(),
            line_range: RepositoryCodeRange::new("line_range", range.line_start, range.line_end)
                .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
        });
    }

    Ok(())
}

fn go_import_specs(import_declaration: &str) -> Vec<String> {
    let mut specs = Vec::new();
    let mut search_start = 0usize;
    while let Some((quote_start, quote)) = next_go_import_quote(import_declaration, search_start) {
        let Some(quote_end) = import_declaration[quote_start + quote.len_utf8()..]
            .find(quote)
            .map(|offset| quote_start + quote.len_utf8() + offset)
        else {
            break;
        };
        let import_path = &import_declaration[quote_start + quote.len_utf8()..quote_end];
        if let Some(spec) =
            go_import_spec_before_quote(import_declaration, quote_start, import_path)
        {
            if !specs.contains(&spec) {
                specs.push(spec);
            }
        }
        search_start = quote_end + quote.len_utf8();
    }

    specs
}

fn next_go_import_quote(value: &str, start: usize) -> Option<(usize, char)> {
    value[start..]
        .char_indices()
        .find(|(_, character)| matches!(character, '"' | '`'))
        .map(|(offset, character)| (start + offset, character))
}

fn go_import_spec_before_quote(
    import_declaration: &str,
    quote_start: usize,
    import_path: &str,
) -> Option<String> {
    if import_path.trim().is_empty() {
        return None;
    }
    let prefix_start = import_declaration[..quote_start]
        .rfind(['\n', '(', ';'])
        .map_or(0, |index| index + 1);
    let raw_prefix = import_declaration[prefix_start..quote_start].trim();
    if raw_prefix.contains("//")
        || raw_prefix.starts_with("/*")
        || raw_prefix.rfind("/*") > raw_prefix.rfind("*/")
    {
        return None;
    }
    let prefix = raw_prefix
        .strip_prefix("import")
        .map_or(raw_prefix, str::trim);
    let alias = prefix
        .split_whitespace()
        .last()
        .filter(|value| matches!(*value, "." | "_") || go_identifier(value));

    Some(match alias {
        Some(alias) => format!("{alias} {import_path}"),
        None => import_path.to_owned(),
    })
}

fn go_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_import_node(node: Node<'_>) -> bool {
    match node.kind() {
        "import" => node.is_named(),
        "import_declaration"
        | "import_from_statement"
        | "import_statement"
        | "namespace_use_declaration"
        | "preproc_include"
        | "use_declaration"
        | "using_declaration"
        | "using_directive" => true,
        _ => false,
    }
}
