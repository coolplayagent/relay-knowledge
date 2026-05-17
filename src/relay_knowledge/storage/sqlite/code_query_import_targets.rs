use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{CodeRepositoryStatus, CodeRetrievalRequest, RepositoryCodeRange},
    storage::StorageError,
};

use super::code_query_rows::ImportRow;
use super::code_query_support::{
    candidate_limit, fts_path_filter_sql, fts_values_for_limited, score_text,
    symbol_fts_match_query,
};
use super::required_scope;

pub(super) fn search_imports_by_target_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ImportRow>, StorageError> {
    if !target_symbol_import_query(&request.query) {
        return Ok(Vec::new());
    }
    let symbol_targets = import_target_symbol_matches(connection, status, request)?;
    if symbol_targets.is_empty() {
        return Ok(Vec::new());
    }
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    let target_hints = symbol_targets
        .iter()
        .map(|target| target.target_hint.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    values.extend(target_hints.iter().cloned().map(Value::Text));
    let matched_names_by_hint = symbol_targets.into_iter().fold(
        BTreeMap::<String, Vec<String>>::new(),
        |mut matched, target| {
            matched
                .entry(target.target_hint)
                .or_default()
                .push(target.symbol_name);
            matched
        },
    );
    let placeholders = placeholders(target_hints.len());
    values.push(Value::Integer(candidate_limit(request) as i64));
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND i.target_hint IN ({placeholders})
        ORDER BY i.path ASC, i.line_start ASC
        LIMIT ?
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        let target_hint = row.get::<_, Option<String>>(6)?;
        let matched_symbol_name = target_hint
            .as_ref()
            .and_then(|target_hint| matched_names_by_hint.get(target_hint))
            .map(|names| names.join(" "));
        Ok(ImportRow {
            file_id: row.get(0)?,
            path: row.get(1)?,
            language_id: row.get(2)?,
            module: row.get(3)?,
            matched_symbol_name,
            line_range: RepositoryCodeRange {
                start: row.get(4)?,
                end: row.get(5)?,
            },
            target_hint,
            resolution_state: row.get(7)?,
            confidence_basis_points: row.get(8)?,
            confidence_tier: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn import_target_symbol_matches(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ImportTargetSymbol>, StorageError> {
    let fts_query = symbol_fts_match_query(&request.query);
    let fts_path_filter = fts_path_filter_sql(status, request);
    let sql = format!(
        "
        SELECT path, name, language_id
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND symbol_snapshot_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'symbol'
                {fts_path_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        ORDER BY path ASC, line_start ASC
        LIMIT ?
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request),
            candidate_limit(request),
        )),
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let query = request.query.as_str();
    let mut targets = Vec::new();
    for row in rows {
        let (path, name, language_id) = row?;
        if score_text(query, [name.as_str(), path.as_str()]) <= 0.0 {
            continue;
        }
        for target_hint in import_target_hints_for_symbol(&path, &language_id) {
            let target = ImportTargetSymbol {
                target_hint,
                symbol_name: name.clone(),
            };
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }

    Ok(targets)
}

#[derive(PartialEq, Eq)]
struct ImportTargetSymbol {
    target_hint: String,
    symbol_name: String,
}

fn import_target_hints_for_symbol(path: &str, _language_id: &str) -> Vec<String> {
    let mut target_hints = vec![path.to_owned()];
    if let Some(parent) = parent_dir(path) {
        target_hints.push(parent.to_owned());
    }
    target_hints.sort();
    target_hints.dedup();

    target_hints
}

pub(super) fn target_symbol_import_query(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && !trimmed.contains('/')
        && !trimmed.contains('\\')
        && !trimmed.contains('.')
}

fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}
