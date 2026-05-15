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

#[cfg(test)]
mod tests {
    use crate::domain::{RepositoryCodeRange, RepositoryCodeReferenceRecord};

    use super::*;

    #[test]
    fn same_path_overloads_remain_ambiguous_even_when_one_contains_call_line() {
        let mut symbols = vec![
            symbol("sym-int", "src/overload.cpp", "run"),
            symbol("sym-double", "src/overload.cpp", "run"),
        ];
        symbols[0].line_range = RepositoryCodeRange { start: 1, end: 3 };
        symbols[1].line_range = RepositoryCodeRange { start: 5, end: 5 };
        let mut references = vec![reference("ref-call", "src/overload.cpp", "run")];
        references[0].line_range = RepositoryCodeRange { start: 2, end: 2 };

        resolve_reference_targets(&symbols, &mut references);

        assert_eq!(references[0].target_symbol_snapshot_id, None);
        assert_eq!(references[0].resolution_state, "ambiguous");
    }

    fn symbol(id: &str, path: &str, name: &str) -> RepositoryCodeSymbolRecord {
        RepositoryCodeSymbolRecord {
            repository_id: "repo".to_owned(),
            source_scope: "git_snapshot:test".to_owned(),
            symbol_snapshot_id: id.to_owned(),
            canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
            file_id: format!("file-{id}"),
            path: path.to_owned(),
            language_id: "cpp".to_owned(),
            name: name.to_owned(),
            qualified_name: format!("{}::{name}", path.replace('/', "::")),
            kind: "function".to_owned(),
            signature: format!("void {name}()"),
            doc_comment: None,
            byte_range: RepositoryCodeRange { start: 0, end: 8 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }
    }

    fn reference(id: &str, path: &str, name: &str) -> RepositoryCodeReferenceRecord {
        RepositoryCodeReferenceRecord {
            repository_id: "repo".to_owned(),
            source_scope: "git_snapshot:test".to_owned(),
            reference_id: id.to_owned(),
            file_id: format!("file-{id}"),
            path: path.to_owned(),
            name: name.to_owned(),
            kind: "call".to_owned(),
            target_symbol_snapshot_id: None,
            target_hint: Some(name.to_owned()),
            resolution_state: "unresolved".to_owned(),
            confidence_basis_points: 2_500,
            confidence_tier: "ambiguous".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 8 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
        }
    }
}
