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

    resolve_reference_target(
        reference.name.as_str(),
        by_name
            .get(reference.name.as_str())
            .map(std::vec::Vec::as_slice),
        by_name_and_path
            .get(&(reference.name.as_str(), reference.path.as_str()))
            .map(std::vec::Vec::as_slice),
    )
}

fn resolve_call_reference_target<'a>(
    reference: &RepositoryCodeReferenceRecord,
    by_name: &BTreeMap<&str, Vec<&'a RepositoryCodeSymbolRecord>>,
    by_name_and_path: &BTreeMap<(&str, &str), Vec<&'a RepositoryCodeSymbolRecord>>,
) -> Resolution<'a> {
    let mut deferred_resolution = None;
    let mut ambiguous_target_hint = None;
    for candidate in call_target_name_candidates(&reference.name, &reference.path) {
        let target_hint = call_target_hint(&reference.name, &candidate);
        match resolve_call_target(
            &candidate,
            by_name.get(candidate.as_str()).map(std::vec::Vec::as_slice),
            by_name_and_path
                .get(&(candidate.as_str(), reference.path.as_str()))
                .map(std::vec::Vec::as_slice),
        ) {
            Resolution::Ambiguous(_) => {
                if let Some(symbol) = unique_preferred_callable(
                    by_name.get(candidate.as_str()).map(std::vec::Vec::as_slice),
                ) {
                    return Resolution::Resolved(symbol, target_hint);
                }
                ambiguous_target_hint.get_or_insert(target_hint);
            }
            Resolution::Resolved(symbol, _) => {
                if callable_definition_symbol(&symbol.kind, &symbol.signature) {
                    return Resolution::Resolved(symbol, target_hint);
                }
                deferred_resolution.get_or_insert((symbol, target_hint));
            }
            Resolution::Unresolved => {}
        }
    }

    if let Some((symbol, target_hint)) = deferred_resolution {
        return Resolution::Resolved(symbol, target_hint);
    }

    ambiguous_target_hint.map_or(Resolution::Unresolved, Resolution::Ambiguous)
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

    #[test]
    fn call_resolution_prefers_implementation_over_header_declaration() {
        let mut symbols = vec![
            symbol("header-declaration", "include/rk_bridge.h", "rk_c_decode"),
            symbol("source-definition", "src/c_entry.c", "rk_c_decode"),
        ];
        symbols[0].kind = "function_declaration".to_owned();
        let mut references = vec![reference("cpp-call", "src/cpp_bridge.cpp", "rk_c_decode")];

        resolve_reference_targets(&symbols, &mut references);

        assert_eq!(
            references[0].target_symbol_snapshot_id.as_deref(),
            Some("source-definition")
        );
        assert_eq!(references[0].target_hint.as_deref(), Some("rk_c_decode"));
        assert_eq!(references[0].resolution_state, "resolved");
    }

    #[test]
    fn call_resolution_follows_cgo_and_ffi_aliases_to_c_symbols() {
        let symbols = vec![symbol("c-definition", "src/c_entry.c", "rk_c_decode")];
        let mut references = vec![
            reference("go-cgo-call", "bridge/go_bridge.go", "C.rk_c_decode"),
            reference(
                "rust-ffi-call",
                "crates/rust_bridge/src/lib.rs",
                "ffi::rk_c_decode",
            ),
        ];

        resolve_reference_targets(&symbols, &mut references);

        assert!(references.iter().all(|reference| {
            reference.target_symbol_snapshot_id.as_deref() == Some("c-definition")
                && reference.resolution_state == "resolved"
        }));
        assert_eq!(references[0].target_hint.as_deref(), Some("C.rk_c_decode"));
        assert_eq!(
            references[1].target_hint.as_deref(),
            Some("ffi::rk_c_decode")
        );
    }

    #[test]
    fn call_resolution_does_not_leaf_alias_ordinary_namespaced_calls() {
        let symbols = vec![symbol("plain-connect", "src/socket.c", "connect")];
        let mut references = vec![
            reference(
                "rust-module-call",
                "crates/app/src/lib.rs",
                "module::connect",
            ),
            reference(
                "rust-module-sys-call",
                "crates/app/src/lib.rs",
                "module::sys::connect",
            ),
            reference("rust-c-member-call", "crates/app/src/lib.rs", "C.connect"),
            reference("go-object-c-call", "bridge/go_bridge.go", "obj.C.connect"),
        ];

        resolve_reference_targets(&symbols, &mut references);

        assert!(references.iter().all(|reference| {
            reference.target_symbol_snapshot_id.is_none()
                && reference.resolution_state == "unresolved"
        }));
        assert_eq!(
            references[0].target_hint.as_deref(),
            Some("module::connect")
        );
        assert_eq!(
            references[1].target_hint.as_deref(),
            Some("module::sys::connect")
        );
        assert_eq!(references[2].target_hint.as_deref(), Some("C.connect"));
        assert_eq!(references[3].target_hint.as_deref(), Some("obj.C.connect"));
    }

    #[test]
    fn call_resolution_ignores_unique_non_callable_leaf_targets() {
        let mut symbols = vec![symbol("constant-connect", "src/constants.rs", "connect")];
        symbols[0].kind = "constant".to_owned();
        let mut references = vec![reference("ffi-connect", "src/lib.rs", "ffi::connect")];

        resolve_reference_targets(&symbols, &mut references);

        assert_eq!(references[0].target_symbol_snapshot_id, None);
        assert_eq!(references[0].target_hint.as_deref(), Some("ffi::connect"));
        assert_eq!(references[0].resolution_state, "unresolved");
    }

    #[test]
    fn call_resolution_continues_to_leaf_after_scoped_ambiguity() {
        let mut symbols = vec![
            symbol("ffi-declaration-a", "src/bindings_a.rs", "ffi::rk_c_decode"),
            symbol("ffi-declaration-b", "src/bindings_b.rs", "ffi::rk_c_decode"),
            symbol("c-definition", "src/c_entry.c", "rk_c_decode"),
        ];
        symbols[0].kind = "function_declaration".to_owned();
        symbols[1].kind = "function_declaration".to_owned();
        let mut references = vec![reference("ffi-call", "src/lib.rs", "ffi::rk_c_decode")];

        resolve_reference_targets(&symbols, &mut references);

        assert_eq!(
            references[0].target_symbol_snapshot_id.as_deref(),
            Some("c-definition")
        );
        assert_eq!(
            references[0].target_hint.as_deref(),
            Some("ffi::rk_c_decode")
        );
        assert_eq!(references[0].resolution_state, "resolved");
    }

    #[test]
    fn call_resolution_prefers_leaf_definition_over_unique_scoped_declaration() {
        let mut symbols = vec![
            symbol("ffi-declaration", "src/bindings.rs", "ffi::rk_c_decode"),
            symbol("c-definition", "src/c_entry.c", "rk_c_decode"),
        ];
        symbols[0].kind = "function_declaration".to_owned();
        let mut references = vec![reference("ffi-call", "src/lib.rs", "ffi::rk_c_decode")];

        resolve_reference_targets(&symbols, &mut references);

        assert_eq!(
            references[0].target_symbol_snapshot_id.as_deref(),
            Some("c-definition")
        );
        assert_eq!(
            references[0].target_hint.as_deref(),
            Some("ffi::rk_c_decode")
        );
        assert_eq!(references[0].resolution_state, "resolved");
    }

    #[test]
    fn call_resolution_treats_signature_only_functions_as_declarations() {
        let mut symbols = vec![
            symbol("ffi-declaration", "src/bindings.rs", "ffi::rk_c_decode"),
            symbol("c-definition", "src/c_entry.c", "rk_c_decode"),
        ];
        symbols[0].signature = "fn rk_c_decode(input: *const u8);".to_owned();
        let mut references = vec![reference("ffi-call", "src/lib.rs", "ffi::rk_c_decode")];

        resolve_reference_targets(&symbols, &mut references);

        assert_eq!(
            references[0].target_symbol_snapshot_id.as_deref(),
            Some("c-definition")
        );
        assert_eq!(
            references[0].target_hint.as_deref(),
            Some("ffi::rk_c_decode")
        );
        assert_eq!(references[0].resolution_state, "resolved");
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
