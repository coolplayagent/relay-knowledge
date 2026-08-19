//! Materializes finalized call edges and their searchable projection.

use std::collections::HashMap;

use rusqlite::{Transaction, params, params_from_iter, types::Value};

use super::{
    search_documents,
    symbols::{self, SymbolKey},
};
use crate::storage::StorageError;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(super) fn rebuild(
    transaction: &Transaction<'_>,
    source_scope: &str,
    repository_id: &str,
    symbol_cache: &mut Option<Vec<SymbolKey>>,
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM code_repository_calls WHERE source_scope = ?1",
        params![source_scope],
    )?;
    let mut by_path = HashMap::<&str, Vec<&SymbolKey>>::new();
    let mut by_symbol_id = HashMap::<&str, &SymbolKey>::new();
    for symbol in symbols::load_once(transaction, source_scope, symbol_cache)? {
        by_path
            .entry(symbol.path.as_str())
            .or_default()
            .push(symbol);
        by_symbol_id.insert(symbol.symbol_snapshot_id.as_str(), symbol);
    }
    let mut insert_call = transaction.prepare(
        "
        INSERT INTO code_repository_calls (
            repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id,
            caller_name, callee_symbol_snapshot_id, callee_name, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
    )?;
    let mut select_references = transaction.prepare(
        "
        SELECT reference_id, file_id, path, name, line_start, line_end,
               target_symbol_snapshot_id, target_hint, resolution_state,
               confidence_basis_points, confidence_tier
        FROM code_repository_references
        WHERE source_scope = ?1 AND kind = 'call'
        ",
    )?;
    let mut references = select_references.query(params![source_scope])?;
    while let Some(row) = references.next()? {
        let reference = ReferenceKey::from_row(row)?;
        let caller = caller_for_line(by_path.get(reference.path.as_str()), reference.line_start);
        let callee = reference
            .target_symbol_snapshot_id
            .as_deref()
            .and_then(|symbol_id| by_symbol_id.get(symbol_id).copied());
        let callee_name = callee
            .map(|symbol| symbol.name.as_str())
            .or(reference.target_hint.as_deref())
            .unwrap_or(reference.name.as_str());
        let call_id = stable_id(
            "call",
            [
                repository_id,
                source_scope,
                reference.reference_id.as_str(),
                reference.path.as_str(),
                reference.name.as_str(),
                &reference.line_start.to_string(),
            ],
        );
        insert_call.execute(params![
            repository_id,
            source_scope,
            call_id,
            reference.file_id.as_str(),
            reference.path.as_str(),
            caller.map(|symbol| symbol.symbol_snapshot_id.as_str()),
            caller.map(|symbol| symbol.name.as_str()),
            reference.target_symbol_snapshot_id.as_deref(),
            callee_name,
            reference.target_hint.as_deref(),
            reference.resolution_state.as_str(),
            reference.confidence_basis_points,
            reference.confidence_tier.as_str(),
            reference.line_start,
            reference.line_end,
        ])?;
    }

    search_documents::rebuild_call_search_documents(transaction, source_scope)
}

/// Path-scoped variant of [`rebuild`]: only deletes and rebuilds call edges
/// whose caller `path` is in `affected_paths`.  The symbol index still
/// loads ALL symbols because a call in an affected path may reference a
/// callee symbol in an unchanged path.
pub(super) fn rebuild_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    repository_id: &str,
    affected_paths: &[&str],
    symbol_cache: &mut Option<Vec<SymbolKey>>,
) -> Result<(), StorageError> {
    let mut paths = affected_paths.to_vec();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(());
    }
    for path_chunk in paths.chunks(500) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut delete_values = Vec::with_capacity(path_chunk.len() + 1);
        delete_values.push(Value::Text(source_scope.to_owned()));
        delete_values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        transaction.execute(
            &format!(
                "DELETE FROM code_repository_calls WHERE source_scope = ? AND path IN ({placeholders})"
            ),
            params_from_iter(delete_values),
        )?;
    }
    let mut by_path = HashMap::<&str, Vec<&SymbolKey>>::new();
    let mut by_symbol_id = HashMap::<&str, &SymbolKey>::new();
    for symbol in symbols::load_once(transaction, source_scope, symbol_cache)? {
        by_path
            .entry(symbol.path.as_str())
            .or_default()
            .push(symbol);
        by_symbol_id.insert(symbol.symbol_snapshot_id.as_str(), symbol);
    }
    let mut insert_call = transaction.prepare(
        "
        INSERT INTO code_repository_calls (
            repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id,
            caller_name, callee_symbol_snapshot_id, callee_name, target_hint,
            resolution_state, confidence_basis_points, confidence_tier, line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
    )?;
    for path_chunk in paths.chunks(500) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut select_values = Vec::with_capacity(path_chunk.len() + 1);
        select_values.push(Value::Text(source_scope.to_owned()));
        select_values.extend(path_chunk.iter().map(|p| Value::Text((*p).to_owned())));
        let mut select_references = transaction.prepare(&format!(
            "
                SELECT reference_id, file_id, path, name, line_start, line_end,
                       target_symbol_snapshot_id, target_hint, resolution_state,
                       confidence_basis_points, confidence_tier
                FROM code_repository_references
                WHERE source_scope = ? AND kind = 'call' AND path IN ({placeholders})
                "
        ))?;
        let mut references = select_references.query(params_from_iter(select_values))?;
        while let Some(row) = references.next()? {
            let reference = ReferenceKey::from_row(row)?;
            let caller =
                caller_for_line(by_path.get(reference.path.as_str()), reference.line_start);
            let callee = reference
                .target_symbol_snapshot_id
                .as_deref()
                .and_then(|symbol_id| by_symbol_id.get(symbol_id).copied());
            let callee_name = callee
                .map(|symbol| symbol.name.as_str())
                .or(reference.target_hint.as_deref())
                .unwrap_or(reference.name.as_str());
            let call_id = stable_id(
                "call",
                [
                    repository_id,
                    source_scope,
                    reference.reference_id.as_str(),
                    reference.path.as_str(),
                    reference.name.as_str(),
                    &reference.line_start.to_string(),
                ],
            );
            insert_call.execute(params![
                repository_id,
                source_scope,
                call_id,
                reference.file_id.as_str(),
                reference.path.as_str(),
                caller.map(|symbol| symbol.symbol_snapshot_id.as_str()),
                caller.map(|symbol| symbol.name.as_str()),
                reference.target_symbol_snapshot_id.as_deref(),
                callee_name,
                reference.target_hint.as_deref(),
                reference.resolution_state.as_str(),
                reference.confidence_basis_points,
                reference.confidence_tier.as_str(),
                reference.line_start,
                reference.line_end,
            ])?;
        }
    }

    search_documents::rebuild_call_search_documents_for_paths(transaction, source_scope, &paths)
}

pub(super) fn caller_for_line<'a>(
    symbols: Option<&Vec<&'a SymbolKey>>,
    line: u32,
) -> Option<&'a SymbolKey> {
    let symbols = symbols?;
    let candidate_end = symbols.partition_point(|symbol| symbol.line_range.start <= line);
    symbols[..candidate_end]
        .iter()
        .rev()
        .find(|symbol| symbol.line_range.end >= line)
        .copied()
}

#[derive(Debug)]
struct ReferenceKey {
    reference_id: String,
    file_id: String,
    path: String,
    name: String,
    line_start: u32,
    line_end: u32,
    target_symbol_snapshot_id: Option<String>,
    target_hint: Option<String>,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
}

impl ReferenceKey {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            reference_id: row.get(0)?,
            file_id: row.get(1)?,
            path: row.get(2)?,
            name: row.get(3)?,
            line_start: row.get(4)?,
            line_end: row.get(5)?,
            target_symbol_snapshot_id: row.get(6)?,
            target_hint: row.get(7)?,
            resolution_state: row.get(8)?,
            confidence_basis_points: row.get(9)?,
            confidence_tier: row.get(10)?,
        })
    }
}

fn stable_id<'a>(prefix: &str, parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }

    format!("{prefix}:{:016x}", stable_hash64(&bytes))
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}
