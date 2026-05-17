use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{CodeRepositoryStatus, CodeRetrievalRequest, RepositoryCodeRange},
    storage::StorageError,
};

use super::code_query_rows::ImportRow;
use super::code_query_support::{
    candidate_limit, language_filter_sql_for_column, path_filter_sql_for_column,
    push_language_filter_values, push_path_filter_values, score_text, symbol_fts_match_query,
};
use super::required_scope;

const SQLITE_BIND_BATCH_SIZE: usize = 500;

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
    let target_hints = symbol_targets
        .iter()
        .map(|target| target.target_hint.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
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

    let mut rows = Vec::new();
    for target_hint_chunk in target_hints.chunks(SQLITE_BIND_BATCH_SIZE) {
        rows.extend(search_imports_by_target_hint_chunk(
            connection,
            status,
            request,
            target_hint_chunk,
            &matched_names_by_hint,
        )?);
    }

    Ok(rows)
}

fn search_imports_by_target_hint_chunk(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    target_hints: &[String],
    matched_names_by_hint: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<ImportRow>, StorageError> {
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(target_hints.iter().cloned().map(Value::Text));
    let placeholders = placeholders(target_hints.len());
    let import_path_filter = path_filter_sql_for_column("i.path", status, request);
    let import_language_filter = language_filter_sql_for_column("f.language_id", status, request);
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
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
          {import_path_filter}
          {import_language_filter}
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
    let sql = "
        SELECT path, name, language_id
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND symbol_snapshot_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'symbol'
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
        )
        ORDER BY path ASC, line_start ASC
        LIMIT ?
        ";
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(
        params_from_iter(symbol_target_fts_values_for_limited(
            required_scope(status)?,
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
        if !symbol_matches_import_target_query(query, &name, &path) {
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

fn symbol_target_fts_values_for_limited(
    source_scope: &str,
    fts_query: &str,
    fts_limit: usize,
    limit: usize,
) -> Vec<Value> {
    vec![
        Value::Text(source_scope.to_owned()),
        Value::Text(fts_query.to_owned()),
        Value::Text(source_scope.to_owned()),
        Value::Integer(fts_limit as i64),
        Value::Integer(limit as i64),
    ]
}

#[derive(PartialEq, Eq)]
struct ImportTargetSymbol {
    target_hint: String,
    symbol_name: String,
}

fn import_target_hints_for_symbol(path: &str, language_id: &str) -> Vec<String> {
    let mut target_hints = Vec::new();
    push_target_hint(&mut target_hints, path.to_owned());
    push_target_hint(&mut target_hints, strip_source_root(path).to_owned());
    if language_id == "go" {
        push_target_hint(&mut target_hints, strip_go_source_root(path).to_owned());
    }
    if let Some(parent) = parent_dir(path) {
        push_target_hint(&mut target_hints, parent.to_owned());
        push_target_hint(&mut target_hints, strip_source_root(parent).to_owned());
        if language_id == "go" {
            push_target_hint(&mut target_hints, strip_go_source_root(parent).to_owned());
        }
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
        && !query_contains_file_extension(trimmed)
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

fn symbol_matches_import_target_query(query: &str, name: &str, path: &str) -> bool {
    score_text(query, [name, path]) > 0.0
        || query_identifier_terms(query)
            .last()
            .is_some_and(|term| term.eq_ignore_ascii_case(name))
}

fn query_identifier_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|term| !term.is_empty())
        .map(str::to_owned)
        .collect()
}

fn query_contains_file_extension(query: &str) -> bool {
    query.split_whitespace().any(|term| {
        let term = term.trim_matches(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
        });
        let Some((stem, extension)) = term.rsplit_once('.') else {
            return false;
        };
        !stem.is_empty() && file_extension_is_path_like(extension)
    })
}

fn file_extension_is_path_like(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cs"
            | "go"
            | "gradle"
            | "h"
            | "hh"
            | "hpp"
            | "hxx"
            | "java"
            | "js"
            | "json"
            | "jsx"
            | "kt"
            | "md"
            | "php"
            | "py"
            | "rb"
            | "rs"
            | "scala"
            | "sh"
            | "swift"
            | "ts"
            | "tsx"
            | "txt"
            | "xml"
            | "yaml"
            | "yml"
    )
}

fn push_target_hint(target_hints: &mut Vec<String>, target_hint: String) {
    if !target_hint.is_empty() && !target_hints.contains(&target_hint) {
        target_hints.push(target_hint);
    }
}

fn strip_source_root(path: &str) -> &str {
    for prefix in [
        "src/main/java/",
        "src/test/java/",
        "src/main/kotlin/",
        "src/test/kotlin/",
        "src/main/scala/",
        "src/test/scala/",
        "src/main/groovy/",
        "src/test/groovy/",
        "src/",
    ] {
        if let Some(stripped) = path.strip_prefix(prefix) {
            return stripped;
        }
    }

    path
}

fn strip_go_source_root(path: &str) -> &str {
    for prefix in ["staging/src/", "vendor/", "src/"] {
        if let Some(stripped) = path.strip_prefix(prefix) {
            return stripped;
        }
    }

    path
}
