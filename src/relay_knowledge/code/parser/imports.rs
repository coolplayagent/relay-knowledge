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
    content: &str,
    root: Node<'_>,
) -> Result<Vec<CodeImportRecord>, CodeIndexError> {
    let mut imports = Vec::new();
    let mut stack = Vec::with_capacity(root.child_count().saturating_add(1));
    stack.push(root);
    while let Some(node) = stack.pop() {
        if is_import_node(node) {
            let module = compact_whitespace(&node_text(content, node));
            let range = syntax_range(node);
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
                line_range: RepositoryCodeRange::new(
                    "line_range",
                    range.line_start,
                    range.line_end,
                )
                .map_err(|error| CodeIndexError::InvalidInput(error.to_string()))?,
            });
        }
        push_children_reverse(node, &mut stack);
    }

    Ok(imports)
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
