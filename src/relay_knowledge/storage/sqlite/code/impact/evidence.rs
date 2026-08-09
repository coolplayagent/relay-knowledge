//! Bounded SQLite chunk, caller, and importer evidence retrieval for code impact.

use std::collections::BTreeSet;

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeImpactRequest, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    super::query::{HitParts, chunk_layers, hit_from_parts, required_scope},
    path_selection::impact_row_allowed,
    seed::module_import_matches,
};

const SQLITE_EXPRESSION_BATCH_SIZE: usize = 250;

pub(super) fn chunks_for_paths(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &BTreeSet<String>,
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let path_values = paths.iter().cloned().collect::<Vec<_>>();
    let path_clause = placeholders(path_values.len());
    let sql = format!(
        "
        SELECT c.file_id, c.path, c.language_id, c.content, c.byte_start, c.byte_end,
               c.line_start, c.line_end, c.symbol_snapshot_id,
               symbol.canonical_symbol_id, f.parse_status, f.degraded_reason, f.is_generated
        FROM code_repository_chunks c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        LEFT JOIN code_repository_symbols symbol
            ON symbol.source_scope = c.source_scope
           AND symbol.symbol_snapshot_id = c.symbol_snapshot_id
        WHERE c.source_scope = ?1
          AND c.path IN ({path_clause})
        ORDER BY c.path ASC, c.line_start ASC
        ",
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(path_values.into_iter().map(Value::Text));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok(ImpactChunkRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            content: row.get(3)?,
            byte_range: RepositoryCodeRange {
                start: row.get(4)?,
                end: row.get(5)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(6)?,
                end: row.get(7)?,
            },
            symbol_snapshot_id: row.get(8)?,
            canonical_symbol_id: row.get(9)?,
            parse_status: row.get(10)?,
            degraded_reason: row.get(11)?,
            is_generated: row.get::<_, i64>(12)? != 0,
        })
    })?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| impact_row_allowed(&row.path, &row.language_id, status, request))
        .map(|row| {
            hit_from_parts(
                status,
                HitParts {
                    path: row.path,
                    language_id: row.language_id,
                    byte_range: row.byte_range,
                    line_range: row.line_range,
                    symbol_snapshot_id: row.symbol_snapshot_id,
                    canonical_symbol_id: row.canonical_symbol_id,
                    file_id: Some(row.file_id),
                    retrieval_layers: chunk_layers(&row.parse_status),
                    score: 4.0,
                    excerpt: row.content,
                    is_generated: row.is_generated,
                    degraded_reason: row.degraded_reason,
                    edge_kind: None,
                    edge_resolution_state: None,
                    edge_target_hint: None,
                    edge_confidence_basis_points: None,
                    edge_confidence_tier: None,
                },
            )
        })
        .collect())
}

pub(super) fn callers_for_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    symbol_ids: &[String],
    deleted_symbol_names: &[String],
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if symbol_ids.is_empty() && deleted_symbol_names.is_empty() {
        return Ok(Vec::new());
    }
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    let mut filters = Vec::new();
    if !symbol_ids.is_empty() {
        filters.push(format!(
            "c.callee_symbol_snapshot_id IN ({})",
            placeholders(symbol_ids.len())
        ));
        values.extend(symbol_ids.iter().cloned().map(Value::Text));
    }
    if !deleted_symbol_names.is_empty() {
        filters.push(format!(
            "(c.callee_symbol_snapshot_id IS NULL AND c.callee_name IN ({}))",
            placeholders(deleted_symbol_names.len())
        ));
        values.extend(deleted_symbol_names.iter().cloned().map(Value::Text));
    }
    let sql = format!(
        "
        SELECT c.file_id, c.path, f.language_id, c.caller_symbol_snapshot_id,
               c.caller_name, c.callee_symbol_snapshot_id, c.callee_name,
               c.line_start, c.line_end, c.target_hint, c.resolution_state,
               c.confidence_basis_points, c.confidence_tier, caller.canonical_symbol_id,
               f.is_generated
        FROM code_repository_calls c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        LEFT JOIN code_repository_symbols caller
            ON caller.source_scope = c.source_scope
           AND caller.symbol_snapshot_id = c.caller_symbol_snapshot_id
        WHERE c.source_scope = ?1
          AND ({})
        ORDER BY c.path ASC, c.line_start ASC
        ",
        filters.join(" OR ")
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok(ImpactCallRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            caller_symbol_snapshot_id: row.get(3)?,
            caller_name: row.get(4)?,
            callee_symbol_snapshot_id: row.get(5)?,
            callee_name: row.get(6)?,
            line_range: RepositoryCodeRange {
                start: row.get(7)?,
                end: row.get(8)?,
            },
            target_hint: row.get(9)?,
            resolution_state: row.get(10)?,
            confidence_basis_points: row.get(11)?,
            confidence_tier: row.get(12)?,
            caller_canonical_symbol_id: row.get(13)?,
            is_generated: row.get::<_, i64>(14)? != 0,
        })
    })?;
    let symbol_set = symbol_ids.iter().collect::<BTreeSet<_>>();
    let deleted_name_set = deleted_symbol_names.iter().collect::<BTreeSet<_>>();
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| impact_row_allowed(&row.path, &row.language_id, status, request))
        .filter(|row| {
            row.callee_symbol_snapshot_id
                .as_ref()
                .is_some_and(|symbol_id| symbol_set.contains(symbol_id))
                || (row.callee_symbol_snapshot_id.is_none()
                    && deleted_name_set.contains(&row.callee_name))
        })
        .map(|row| {
            let caller = row.caller_name.unwrap_or_else(|| "<module>".to_owned());
            hit_from_parts(
                status,
                HitParts {
                    path: row.path,
                    language_id: row.language_id,
                    byte_range: RepositoryCodeRange { start: 0, end: 0 },
                    line_range: row.line_range,
                    symbol_snapshot_id: row.caller_symbol_snapshot_id,
                    canonical_symbol_id: row.caller_canonical_symbol_id,
                    file_id: Some(row.file_id),
                    retrieval_layers: vec![CodeRetrievalLayer::CallGraph],
                    score: 2.5,
                    excerpt: format!("{caller} calls {}", row.callee_name),
                    is_generated: row.is_generated,
                    degraded_reason: None,
                    edge_kind: Some("call".to_owned()),
                    edge_resolution_state: Some(row.resolution_state),
                    edge_target_hint: row.target_hint,
                    edge_confidence_basis_points: Some(row.confidence_basis_points),
                    edge_confidence_tier: Some(row.confidence_tier),
                },
            )
        })
        .collect())
}

pub(super) fn importers_for_modules(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    modules: &[String],
    request: &CodeImpactRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if modules.is_empty() {
        return Ok(Vec::new());
    }
    let module_patterns = modules
        .iter()
        .map(|module| format!("%{module}%"))
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for patterns in module_patterns.chunks(SQLITE_EXPRESSION_BATCH_SIZE) {
        rows.extend(import_rows_for_patterns(connection, status, patterns)?);
    }

    Ok(rows
        .into_iter()
        .filter(|row| impact_row_allowed(&row.path, &row.language_id, status, request))
        .filter(|row| {
            modules
                .iter()
                .any(|module| module_import_matches(&row.module, module))
        })
        .map(|row| {
            hit_from_parts(
                status,
                HitParts {
                    path: row.path,
                    language_id: row.language_id,
                    byte_range: RepositoryCodeRange { start: 0, end: 0 },
                    line_range: row.line_range,
                    symbol_snapshot_id: None,
                    canonical_symbol_id: None,
                    file_id: Some(row.file_id),
                    retrieval_layers: vec![CodeRetrievalLayer::ImportGraph],
                    score: 2.0,
                    excerpt: row.module,
                    is_generated: row.is_generated,
                    degraded_reason: None,
                    edge_kind: Some("import".to_owned()),
                    edge_resolution_state: Some(row.resolution_state),
                    edge_target_hint: row.target_hint,
                    edge_confidence_basis_points: Some(row.confidence_basis_points),
                    edge_confidence_tier: Some(row.confidence_tier),
                },
            )
        })
        .collect())
}

fn import_rows_for_patterns(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    module_patterns: &[String],
) -> Result<Vec<ImpactImportRow>, StorageError> {
    let module_clause = module_patterns
        .iter()
        .map(|_| "i.module LIKE ?")
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?1
          AND ({module_clause})
        ORDER BY i.path ASC, i.line_start ASC
        ",
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(module_patterns.iter().cloned().map(Value::Text));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok(ImpactImportRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            module: row.get(3)?,
            line_range: RepositoryCodeRange {
                start: row.get(4)?,
                end: row.get(5)?,
            },
            target_hint: row.get(6)?,
            resolution_state: row.get(7)?,
            confidence_basis_points: row.get(8)?,
            confidence_tier: row.get(9)?,
            is_generated: row.get::<_, i64>(10)? != 0,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

struct ImpactChunkRow {
    file_id: String,
    path: String,
    language_id: String,
    content: String,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
    symbol_snapshot_id: Option<String>,
    canonical_symbol_id: Option<String>,
    parse_status: String,
    degraded_reason: Option<String>,
    is_generated: bool,
}

struct ImpactCallRow {
    file_id: String,
    path: String,
    language_id: String,
    caller_symbol_snapshot_id: Option<String>,
    caller_name: Option<String>,
    callee_symbol_snapshot_id: Option<String>,
    callee_name: String,
    line_range: RepositoryCodeRange,
    target_hint: Option<String>,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
    caller_canonical_symbol_id: Option<String>,
    is_generated: bool,
}

struct ImpactImportRow {
    file_id: String,
    path: String,
    language_id: String,
    module: String,
    line_range: RepositoryCodeRange,
    target_hint: Option<String>,
    resolution_state: String,
    confidence_basis_points: u16,
    confidence_tier: String,
    is_generated: bool,
}
