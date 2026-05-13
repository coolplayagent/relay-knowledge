use std::collections::BTreeMap;

use crate::domain::{RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord};

pub(super) fn enrich_symbol_identities(
    repository_id: &str,
    symbols: &mut [RepositoryCodeSymbolRecord],
) {
    let original_names = symbols
        .iter()
        .map(|symbol| {
            (
                symbol.symbol_snapshot_id.clone(),
                symbol.path.clone(),
                symbol.name.clone(),
                symbol.kind.clone(),
                symbol.line_range.start,
                symbol.line_range.end,
                path_prefix(&symbol.qualified_name).to_owned(),
            )
        })
        .collect::<Vec<_>>();

    for symbol in symbols {
        let mut ancestors = original_names
            .iter()
            .filter(|candidate| {
                candidate.1 == symbol.path
                    && candidate.0 != symbol.symbol_snapshot_id
                    && container_kind(&candidate.3)
                    && candidate.4 <= symbol.line_range.start
                    && candidate.5 >= symbol.line_range.end
            })
            .collect::<Vec<_>>();
        ancestors.sort_by(|left, right| {
            left.4
                .cmp(&right.4)
                .then_with(|| right.5.cmp(&left.5))
                .then_with(|| left.2.cmp(&right.2))
        });
        let prefix = path_prefix(&symbol.qualified_name).to_owned();
        let mut segments = ancestors
            .into_iter()
            .map(|ancestor| ancestor.2.clone())
            .collect::<Vec<_>>();
        segments.push(symbol.name.clone());
        symbol.qualified_name = format!("{prefix}::{}", segments.join("."));
        symbol.canonical_symbol_id = format!("repo://{repository_id}/{}", symbol.qualified_name);
    }
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
        .split_once("::")
        .map_or(qualified_name, |(prefix, _)| prefix)
}

fn container_kind(kind: &str) -> bool {
    matches!(kind, "class" | "interface" | "module" | "type")
}
