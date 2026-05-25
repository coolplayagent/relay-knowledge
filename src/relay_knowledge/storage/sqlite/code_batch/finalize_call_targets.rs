use std::collections::BTreeMap;

use rusqlite::{Transaction, params};

use crate::{
    domain::code_call_targets::{
        call_target_name_candidates, callable_definition_symbol_kind, callable_target_symbol_kind,
    },
    storage::StorageError,
};

pub(super) fn resolve_references(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    let index = CallTargetIndex::load(transaction, source_scope)?;
    if index.is_empty() {
        return Ok(());
    }
    let references = load_call_references(transaction, source_scope)?;
    if references.is_empty() {
        return Ok(());
    }
    let mut update = transaction.prepare(
        "
        UPDATE code_repository_references
        SET target_symbol_snapshot_id = ?3,
            target_hint = ?4,
            resolution_state = ?5,
            confidence_basis_points = ?6,
            confidence_tier = ?7
        WHERE source_scope = ?1 AND reference_id = ?2
        ",
    )?;
    for reference in references {
        match index.resolve(&reference.name, &reference.path) {
            TargetResolution::Resolved(symbol, target_hint) => {
                update.execute(params![
                    source_scope,
                    reference.reference_id,
                    symbol.symbol_snapshot_id,
                    target_hint,
                    "resolved",
                    8_000_u16,
                    "inferred"
                ])?;
            }
            TargetResolution::Ambiguous(target_hint) => {
                update.execute(params![
                    source_scope,
                    reference.reference_id,
                    Option::<String>::None,
                    target_hint,
                    "ambiguous",
                    5_000_u16,
                    "ambiguous"
                ])?;
            }
            TargetResolution::Unresolved => {
                update.execute(params![
                    source_scope,
                    reference.reference_id,
                    Option::<String>::None,
                    reference.name,
                    "unresolved",
                    2_500_u16,
                    "ambiguous"
                ])?;
            }
        }
    }

    Ok(())
}

struct CallTargetIndex {
    by_name: BTreeMap<String, Vec<CallTargetSymbol>>,
}

#[derive(Clone)]
struct CallTargetSymbol {
    symbol_snapshot_id: String,
    path: String,
    kind: String,
}

struct CallReference {
    reference_id: String,
    path: String,
    name: String,
}

enum TargetResolution {
    Resolved(CallTargetSymbol, String),
    Ambiguous(String),
    Unresolved,
}

impl CallTargetIndex {
    fn load(transaction: &Transaction<'_>, source_scope: &str) -> Result<Self, StorageError> {
        let mut statement = transaction.prepare(
            "
            SELECT symbol_snapshot_id, path, name, kind
            FROM code_repository_symbols
            WHERE source_scope = ?1
            ",
        )?;
        let rows = statement.query_map(params![source_scope], |row| {
            let name: String = row.get(2)?;
            let symbol = CallTargetSymbol {
                symbol_snapshot_id: row.get(0)?,
                path: row.get(1)?,
                kind: row.get(3)?,
            };
            Ok((name, symbol))
        })?;
        let mut by_name = BTreeMap::<String, Vec<CallTargetSymbol>>::new();
        for row in rows {
            let (name, symbol) = row?;
            by_name.entry(name).or_default().push(symbol);
        }

        Ok(Self { by_name })
    }

    fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    fn resolve(&self, name: &str, reference_path: &str) -> TargetResolution {
        let candidates = call_target_name_candidates(name, reference_path);
        let mut ambiguous_target_hint = None;
        let mut deferred_scoped_resolution = None;
        for (position, candidate) in candidates.iter().enumerate() {
            let target_hint = call_target_hint(name, candidate);
            let has_alias_fallback = position + 1 < candidates.len();
            match self.resolve_candidate(candidate, reference_path) {
                TargetResolution::Unresolved => {}
                TargetResolution::Resolved(symbol, _) => {
                    if has_alias_fallback && !callable_definition_symbol_kind(&symbol.kind) {
                        deferred_scoped_resolution.get_or_insert((symbol, target_hint));
                        continue;
                    }
                    return TargetResolution::Resolved(symbol, target_hint);
                }
                TargetResolution::Ambiguous(_) => {
                    ambiguous_target_hint.get_or_insert(target_hint);
                }
            }
        }

        if let Some(target_hint) = ambiguous_target_hint {
            return TargetResolution::Ambiguous(target_hint);
        }
        deferred_scoped_resolution.map_or(TargetResolution::Unresolved, |(symbol, target_hint)| {
            TargetResolution::Resolved(symbol, target_hint)
        })
    }

    fn resolve_candidate(&self, name: &str, reference_path: &str) -> TargetResolution {
        let Some(symbols) = self.by_name.get(name) else {
            return TargetResolution::Unresolved;
        };
        if let [symbol] = symbols.as_slice() {
            if callable_target_symbol_kind(&symbol.kind) {
                return TargetResolution::Resolved(symbol.clone(), name.to_owned());
            }
            return TargetResolution::Unresolved;
        }
        if !symbols
            .iter()
            .any(|symbol| callable_target_symbol_kind(&symbol.kind))
        {
            return TargetResolution::Unresolved;
        }
        let same_path = symbols
            .iter()
            .filter(|symbol| callable_target_symbol_kind(&symbol.kind))
            .filter(|symbol| symbol.path == reference_path)
            .take(2)
            .cloned()
            .collect::<Vec<_>>();
        if let [symbol] = same_path.as_slice() {
            return TargetResolution::Resolved(symbol.clone(), name.to_owned());
        }
        if let Some(symbol) = unique_preferred_callable(symbols) {
            return TargetResolution::Resolved(symbol, name.to_owned());
        }

        TargetResolution::Ambiguous(name.to_owned())
    }
}

fn call_target_hint(reference_name: &str, candidate: &str) -> String {
    if candidate == reference_name {
        candidate.to_owned()
    } else {
        reference_name.to_owned()
    }
}

fn unique_preferred_callable(symbols: &[CallTargetSymbol]) -> Option<CallTargetSymbol> {
    let definitions = symbols
        .iter()
        .filter(|symbol| callable_definition_symbol_kind(&symbol.kind))
        .take(2)
        .cloned()
        .collect::<Vec<_>>();
    if let [symbol] = definitions.as_slice() {
        return Some(symbol.clone());
    }
    if !definitions.is_empty() {
        return None;
    }
    let callable = symbols
        .iter()
        .filter(|symbol| callable_target_symbol_kind(&symbol.kind))
        .take(2)
        .cloned()
        .collect::<Vec<_>>();
    match callable.as_slice() {
        [symbol] => Some(symbol.clone()),
        _ => None,
    }
}

fn load_call_references(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<Vec<CallReference>, StorageError> {
    let mut statement = transaction.prepare(
        "
        SELECT reference_id, path, name
        FROM code_repository_references
        WHERE source_scope = ?1
          AND kind = 'call'
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(CallReference {
            reference_id: row.get(0)?,
            path: row.get(1)?,
            name: row.get(2)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}
