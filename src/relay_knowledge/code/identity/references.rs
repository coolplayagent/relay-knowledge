use std::collections::BTreeMap;

use crate::domain::{
    RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    code_call_targets::{
        call_target_name_candidates, callable_definition_symbol, callable_target_symbol_kind,
    },
};

pub(in crate::code) fn resolve_reference_targets(
    symbols: &[RepositoryCodeSymbolRecord],
    references: &mut [RepositoryCodeReferenceRecord],
) {
    let mut by_name = BTreeMap::<&str, Vec<&RepositoryCodeSymbolRecord>>::new();
    let mut by_name_and_path = BTreeMap::<(&str, &str), Vec<&RepositoryCodeSymbolRecord>>::new();
    for symbol in symbols {
        by_name.entry(&symbol.name).or_default().push(symbol);
        by_name_and_path
            .entry((symbol.name.as_str(), symbol.path.as_str()))
            .or_default()
            .push(symbol);
    }
    for reference in references {
        reference.target_hint = Some(reference.name.clone());
        match resolve_reference(reference, &by_name, &by_name_and_path) {
            Resolution::Resolved(symbol, target_hint) => {
                reference.target_symbol_snapshot_id = Some(symbol.symbol_snapshot_id.clone());
                reference.target_hint = Some(target_hint);
                reference.resolution_state = "resolved".to_owned();
                reference.confidence_basis_points = 8_000;
                reference.confidence_tier = "inferred".to_owned();
            }
            Resolution::Ambiguous(target_hint) => {
                reference.target_symbol_snapshot_id = None;
                reference.target_hint = Some(target_hint);
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
    Resolved(&'a RepositoryCodeSymbolRecord, String),
    Ambiguous(String),
    Unresolved,
}

fn resolve_reference<'a>(
    reference: &RepositoryCodeReferenceRecord,
    by_name: &BTreeMap<&str, Vec<&'a RepositoryCodeSymbolRecord>>,
    by_name_and_path: &BTreeMap<(&str, &str), Vec<&'a RepositoryCodeSymbolRecord>>,
) -> Resolution<'a> {
    if reference.kind == "call" {
        return resolve_call_reference_target(reference, by_name, by_name_and_path);
    }

    let candidates = by_name
        .get(reference.name.as_str())
        .map(std::vec::Vec::as_slice);
    let same_path_candidates = by_name_and_path
        .get(&(reference.name.as_str(), reference.path.as_str()))
        .map(std::vec::Vec::as_slice);
    if matches!(reference.kind.as_str(), "target" | "variable")
        && make_reference_path(&reference.path)
    {
        return resolve_same_path_reference_target(
            reference.name.as_str(),
            compatible_scoped_symbols(same_path_candidates, &reference.kind).as_deref(),
        );
    }
    if reference.kind == "variable" && cmake_reference_path(&reference.path) {
        return resolve_same_path_reference_target(
            reference.name.as_str(),
            compatible_scoped_symbols(same_path_candidates, &reference.kind).as_deref(),
        );
    }
    if reference.kind == "stage" && dockerfile_reference_path(&reference.path) {
        return resolve_same_path_reference_target(
            reference.name.as_str(),
            compatible_scoped_symbols(same_path_candidates, &reference.kind).as_deref(),
        );
    }
    if reference.kind == "template" {
        return resolve_reference_target(
            reference.name.as_str(),
            compatible_template_symbols(candidates, &reference.path).as_deref(),
            compatible_template_symbols(same_path_candidates, &reference.path).as_deref(),
        );
    }
    if scoped_reference_kind(&reference.kind) {
        return resolve_reference_target(
            reference.name.as_str(),
            compatible_scoped_symbols(candidates, &reference.kind).as_deref(),
            compatible_scoped_symbols(same_path_candidates, &reference.kind).as_deref(),
        );
    }

    resolve_reference_target(reference.name.as_str(), candidates, same_path_candidates)
}

fn scoped_reference_kind(kind: &str) -> bool {
    matches!(kind, "target" | "variable" | "dependency" | "template")
}

fn make_reference_path(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    matches!(file_name, "Makefile" | "GNUmakefile" | "BSDmakefile") || file_name.ends_with(".mk")
}

fn cmake_reference_path(path: &str) -> bool {
    path.rsplit('/').next().unwrap_or(path) == "CMakeLists.txt" || path.ends_with(".cmake")
}

fn dockerfile_reference_path(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    matches!(file_name, "Dockerfile" | "Containerfile")
        || file_name.starts_with("Dockerfile.")
        || file_name.starts_with("Containerfile.")
        || file_name.ends_with(".Dockerfile")
        || file_name.ends_with(".Containerfile")
}

fn compatible_template_symbols<'a>(
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
    reference_path: &str,
) -> Option<Vec<&'a RepositoryCodeSymbolRecord>> {
    let candidates = compatible_scoped_symbols(candidates, "template")?;
    let Some(template_root) = nearest_template_root(reference_path) else {
        return Some(candidates);
    };
    let symbols = candidates
        .into_iter()
        .filter(|symbol| path_in_directory_tree(&symbol.path, template_root))
        .collect::<Vec<_>>();

    (!symbols.is_empty()).then_some(symbols)
}

fn compatible_scoped_symbols<'a>(
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
    reference_kind: &str,
) -> Option<Vec<&'a RepositoryCodeSymbolRecord>> {
    let symbols = candidates?
        .iter()
        .copied()
        .filter(|symbol| scoped_symbol_matches(reference_kind, &symbol.kind))
        .collect::<Vec<_>>();
    (!symbols.is_empty()).then_some(symbols)
}

fn scoped_symbol_matches(reference_kind: &str, symbol_kind: &str) -> bool {
    match reference_kind {
        "dependency" => matches!(symbol_kind, "dependency" | "module"),
        _ => symbol_kind == reference_kind,
    }
}

fn nearest_template_root(path: &str) -> Option<&str> {
    let mut root_end = None;
    let mut offset = 0usize;
    for segment in path.split('/') {
        let end = offset + segment.len();
        if segment == "templates" {
            root_end = Some(end);
        }
        offset = end + 1;
    }

    root_end.map(|end| &path[..end])
}

fn path_in_directory_tree(path: &str, directory: &str) -> bool {
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn resolve_call_reference_target<'a>(
    reference: &RepositoryCodeReferenceRecord,
    by_name: &BTreeMap<&str, Vec<&'a RepositoryCodeSymbolRecord>>,
    by_name_and_path: &BTreeMap<(&str, &str), Vec<&'a RepositoryCodeSymbolRecord>>,
) -> Resolution<'a> {
    let candidates = call_target_name_candidates(&reference.name, &reference.path);
    let mut ambiguous_target_hint = None;
    let mut deferred_resolution = None;
    for (position, candidate) in candidates.iter().enumerate() {
        let target_hint = call_target_hint(&reference.name, candidate);
        let has_alias_fallback = position + 1 < candidates.len();
        match resolve_call_target(
            candidate,
            by_name.get(candidate.as_str()).map(std::vec::Vec::as_slice),
            by_name_and_path
                .get(&(candidate.as_str(), reference.path.as_str()))
                .map(std::vec::Vec::as_slice),
        ) {
            Resolution::Ambiguous(_) => {
                if let Some(symbol) = unique_preferred_callable(
                    by_name.get(candidate.as_str()).map(std::vec::Vec::as_slice),
                ) {
                    if has_alias_fallback
                        && !callable_definition_symbol(&symbol.kind, &symbol.signature)
                    {
                        deferred_resolution.get_or_insert((symbol, target_hint));
                        continue;
                    }
                    return Resolution::Resolved(symbol, target_hint);
                }
                ambiguous_target_hint.get_or_insert(target_hint);
            }
            Resolution::Resolved(symbol, _) => {
                if has_alias_fallback
                    && !callable_definition_symbol(&symbol.kind, &symbol.signature)
                {
                    deferred_resolution.get_or_insert((symbol, target_hint));
                    continue;
                }
                return Resolution::Resolved(symbol, target_hint);
            }
            Resolution::Unresolved => {}
        }
    }

    if let Some(target_hint) = ambiguous_target_hint {
        return Resolution::Ambiguous(target_hint);
    }
    deferred_resolution.map_or(Resolution::Unresolved, |(symbol, target_hint)| {
        Resolution::Resolved(symbol, target_hint)
    })
}

fn call_target_hint(reference_name: &str, candidate: &str) -> String {
    if candidate == reference_name {
        candidate.to_owned()
    } else {
        reference_name.to_owned()
    }
}

fn resolve_reference_target<'a>(
    target_hint: &str,
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
    same_path_candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
) -> Resolution<'a> {
    let Some(candidates) = candidates else {
        return Resolution::Unresolved;
    };
    if candidates.len() == 1 {
        return Resolution::Resolved(candidates[0], target_hint.to_owned());
    }

    if let Some(same_path) = same_path_candidates.and_then(unique_candidate) {
        return Resolution::Resolved(same_path, target_hint.to_owned());
    }

    Resolution::Ambiguous(target_hint.to_owned())
}

fn resolve_same_path_reference_target<'a>(
    target_hint: &str,
    same_path_candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
) -> Resolution<'a> {
    let Some(candidates) = same_path_candidates else {
        return Resolution::Unresolved;
    };
    if let Some(same_path) = unique_candidate(candidates) {
        return Resolution::Resolved(same_path, target_hint.to_owned());
    }

    Resolution::Ambiguous(target_hint.to_owned())
}

fn resolve_call_target<'a>(
    target_hint: &str,
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
    same_path_candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
) -> Resolution<'a> {
    let Some(candidates) = candidates else {
        return Resolution::Unresolved;
    };
    if !candidates
        .iter()
        .any(|candidate| callable_target_symbol_kind(&candidate.kind))
    {
        return Resolution::Unresolved;
    }
    if candidates.len() == 1 && callable_target_symbol_kind(&candidates[0].kind) {
        return Resolution::Resolved(candidates[0], target_hint.to_owned());
    }

    if let Some(same_path) = same_path_candidates.and_then(unique_callable_candidate) {
        return Resolution::Resolved(same_path, target_hint.to_owned());
    }

    Resolution::Ambiguous(target_hint.to_owned())
}

fn unique_candidate<'a>(
    candidates: &[&'a RepositoryCodeSymbolRecord],
) -> Option<&'a RepositoryCodeSymbolRecord> {
    match candidates {
        [candidate] => Some(*candidate),
        _ => None,
    }
}

fn unique_callable_candidate<'a>(
    candidates: &[&'a RepositoryCodeSymbolRecord],
) -> Option<&'a RepositoryCodeSymbolRecord> {
    let callable = candidates
        .iter()
        .filter(|symbol| callable_target_symbol_kind(&symbol.kind))
        .copied()
        .collect::<Vec<_>>();
    match callable.as_slice() {
        [candidate] => Some(*candidate),
        _ => None,
    }
}

fn unique_preferred_callable<'a>(
    candidates: Option<&[&'a RepositoryCodeSymbolRecord]>,
) -> Option<&'a RepositoryCodeSymbolRecord> {
    let candidates = candidates?;
    let definitions = candidates
        .iter()
        .filter(|symbol| callable_definition_symbol(&symbol.kind, &symbol.signature))
        .copied()
        .collect::<Vec<_>>();
    match definitions.as_slice() {
        [symbol] => return Some(*symbol),
        [_, ..] => return None,
        [] => {}
    }
    let callable = candidates
        .iter()
        .filter(|symbol| callable_target_symbol_kind(&symbol.kind))
        .copied()
        .collect::<Vec<_>>();
    match callable.as_slice() {
        [symbol] => Some(*symbol),
        _ => None,
    }
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod tests;
