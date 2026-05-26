use std::collections::BTreeMap;

use crate::domain::RepositoryCodeSymbolRecord;

pub(in crate::code) fn enrich_symbol_identities(
    repository_id: &str,
    symbols: &mut [RepositoryCodeSymbolRecord],
) {
    let symbol_metadata = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| SymbolIdentityMetadata {
            index,
            path: symbol.path.clone(),
            name: symbol.name.clone(),
            kind: symbol.kind.clone(),
            line_start: symbol.line_range.start,
            line_end: symbol.line_range.end,
            byte_start: symbol.byte_range.start,
            byte_end: symbol.byte_range.end,
            prefix: path_prefix(&symbol.qualified_name).to_owned(),
        })
        .collect::<Vec<_>>();
    let mut by_path = BTreeMap::<&str, Vec<usize>>::new();
    for (metadata_index, metadata) in symbol_metadata.iter().enumerate() {
        by_path
            .entry(metadata.path.as_str())
            .or_default()
            .push(metadata_index);
    }

    for metadata_indices in by_path.values_mut() {
        metadata_indices.sort_by(|left, right| {
            let left = &symbol_metadata[*left];
            let right = &symbol_metadata[*right];
            left.line_start
                .cmp(&right.line_start)
                .then_with(|| left.byte_start.cmp(&right.byte_start))
                .then_with(|| right.line_end.cmp(&left.line_end))
                .then_with(|| right.byte_end.cmp(&left.byte_end))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut container_stack = Vec::<usize>::new();
        for metadata_index in metadata_indices {
            let metadata = &symbol_metadata[*metadata_index];
            while container_stack
                .last()
                .is_some_and(|ancestor| !symbol_contains(&symbol_metadata[*ancestor], metadata))
            {
                container_stack.pop();
            }
            let mut segments = container_stack
                .iter()
                .map(|ancestor| symbol_metadata[*ancestor].name.clone())
                .collect::<Vec<_>>();
            segments.push(metadata.name.clone());
            symbols[metadata.index].qualified_name =
                format!("{}::{}", metadata.prefix, segments.join("."));
            symbols[metadata.index].canonical_symbol_id = format!(
                "repo://{repository_id}/{}",
                symbols[metadata.index].qualified_name
            );
            if container_kind(&metadata.kind) {
                container_stack.push(*metadata_index);
            }
        }
    }
}

struct SymbolIdentityMetadata {
    index: usize,
    path: String,
    name: String,
    kind: String,
    line_start: u32,
    line_end: u32,
    byte_start: u32,
    byte_end: u32,
    prefix: String,
}

fn path_prefix(qualified_name: &str) -> &str {
    qualified_name
        .rsplit_once("::")
        .map_or(qualified_name, |(prefix, _)| prefix)
}

fn container_kind(kind: &str) -> bool {
    matches!(
        kind,
        "class" | "constructor" | "function" | "interface" | "method" | "module" | "type"
    )
}

fn symbol_contains(container: &SymbolIdentityMetadata, symbol: &SymbolIdentityMetadata) -> bool {
    container.line_start <= symbol.line_start
        && container.line_end >= symbol.line_end
        && container.byte_start <= symbol.byte_start
        && container.byte_end >= symbol.byte_end
}
