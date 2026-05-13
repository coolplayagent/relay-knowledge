use std::collections::BTreeMap;

use crate::domain::{RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord};

pub(super) fn enrich_symbol_identities(
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
                .then_with(|| right.line_end.cmp(&left.line_end))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut container_stack = Vec::<usize>::new();
        for metadata_index in metadata_indices {
            let metadata = &symbol_metadata[*metadata_index];
            while container_stack
                .last()
                .is_some_and(|ancestor| symbol_metadata[*ancestor].line_end < metadata.line_end)
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
    prefix: String,
}

pub(super) fn resolve_reference_targets(
    symbols: &[RepositoryCodeSymbolRecord],
    references: &mut [RepositoryCodeReferenceRecord],
) {
    let mut by_name = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
    for symbol in symbols {
        by_name.entry(&symbol.name).or_default().push(symbol);
    }
    for reference in references {
        reference.target_hint = Some(reference.name.clone());
        match resolve_reference_target(
            reference,
            by_name
                .get(reference.name.as_str())
                .map(std::vec::Vec::as_slice),
        ) {
            Resolution::Resolved(symbol) => {
                reference.target_symbol_snapshot_id = Some(symbol.symbol_snapshot_id.clone());
                reference.resolution_state = "resolved".to_owned();
                reference.confidence_basis_points = 8_000;
                reference.confidence_tier = "inferred".to_owned();
            }
            Resolution::Ambiguous => {
                reference.target_symbol_snapshot_id = None;
                reference.resolution_state = "ambiguous".to_owned();
                reference.confidence_basis_points = 5_000;
                reference.confidence_tier = "ambiguous".to_owned();
            }
            Resolution::Unresolved => {
                reference.target_symbol_snapshot_id = None;
                reference.resolution_state = "unresolved".to_owned();
                reference.confidence_basis_points = 2_500;
                reference.confidence_tier = "ambiguous".to_owned();
            }
        }
    }
}

enum Resolution<'a> {
    Resolved(&'a RepositoryCodeSymbolRecord),
    Ambiguous,
    Unresolved,
}

fn resolve_reference_target<'a>(
    reference: &RepositoryCodeReferenceRecord,
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
) -> Resolution<'a> {
    let Some(candidates) = candidates else {
        return Resolution::Unresolved;
    };
    if candidates.len() == 1 {
        return Resolution::Resolved(candidates[0]);
    }

    let same_path = candidates
        .iter()
        .copied()
        .filter(|symbol| symbol.path == reference.path)
        .collect::<Vec<_>>();
    if same_path.len() == 1 {
        return Resolution::Resolved(same_path[0]);
    }

    Resolution::Ambiguous
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
