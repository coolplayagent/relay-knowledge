use std::collections::BTreeMap;

use crate::domain::{RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord};

pub(in crate::code) fn resolve_reference_targets(
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
