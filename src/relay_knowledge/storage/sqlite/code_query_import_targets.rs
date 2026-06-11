use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest, RepositoryCodeRange},
    storage::StorageError,
};

use super::code_query_import_scoring::{
    import_usage_identifier_terms, named_import_binding_terms, named_import_binding_terms_for_query,
};
use super::code_query_rows::ImportRow;
use super::code_query_support::{
    CandidateLayer, candidate_limit, language_filter_sql_for_columns, path_filter_sql_for_column,
    push_language_filter_values, push_path_filter_values, score_text, symbol_fts_match_query,
};
use super::{prepare_code_search_statement, required_scope};

const SQLITE_BIND_BATCH_SIZE: usize = 500;
const MAX_TARGET_SYMBOL_NAMES_PER_IMPORT: usize = 4;
const MAX_IMPORT_USAGE_CONTEXT_CHUNKS_PER_PATH: usize = 64;

pub(super) fn attach_import_target_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    rows: &mut [ImportRow],
) -> Result<(), StorageError> {
    let target_paths = rows
        .iter()
        .filter_map(|row| row.target_hint.as_deref())
        .filter(|target_hint| !target_hint.trim().is_empty())
        .collect::<BTreeSet<_>>();
    if target_paths.is_empty() {
        return Ok(());
    }

    let target_paths = target_paths.into_iter().collect::<Vec<_>>();
    let mut symbols_by_path = BTreeMap::<String, Vec<String>>::new();
    for target_path_chunk in target_paths.chunks(SQLITE_BIND_BATCH_SIZE - 1) {
        for (path, name) in import_target_symbols(connection, status, target_path_chunk)? {
            let names = symbols_by_path.entry(path).or_default();
            if names.len() < MAX_TARGET_SYMBOL_NAMES_PER_IMPORT && !names.contains(&name) {
                names.push(name);
            }
        }
    }

    for row in rows {
        let Some(target_hint) = row.target_hint.as_deref() else {
            continue;
        };
        let Some(names) = symbols_by_path
            .get(target_hint)
            .filter(|names| !names.is_empty())
        else {
            continue;
        };
        row.target_symbol_names = Some(names.join(" "));
    }

    Ok(())
}

pub(super) fn attach_import_query_usage_context(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: &mut [ImportRow],
) -> Result<(), StorageError> {
    if !import_usage_context_needed(request, rows) {
        return Ok(());
    }

    let paths = rows
        .iter()
        .map(|row| row.path.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut content_by_path = BTreeMap::<String, String>::new();
    for path_chunk in paths.chunks(SQLITE_BIND_BATCH_SIZE - 1) {
        for (path, content) in import_context_chunks(connection, status, path_chunk)? {
            let entry = content_by_path.entry(path).or_default();
            if !entry.is_empty() {
                entry.push('\n');
            }
            entry.push_str(&content);
        }
    }

    for row in rows {
        let usage_terms = import_usage_terms_for_row(&request.query, row);
        if usage_terms.is_empty() {
            continue;
        }
        let usage = content_by_path
            .get(&row.path)
            .map(|content| identifier_occurrences(content, &usage_terms))
            .unwrap_or_default();
        let import_line_usage = identifier_occurrences(&row.module, &usage_terms);
        row.same_file_query_usage_count = usage.saturating_sub(import_line_usage);
    }

    Ok(())
}

fn import_usage_terms_for_row(query: &str, row: &ImportRow) -> Vec<String> {
    let symbol_import_query = target_symbol_import_query(query);
    let mut terms = if symbol_import_query {
        query_identifier_terms(query)
            .into_iter()
            .filter(|term| term.len() >= 3)
            .map(|term| term.to_ascii_lowercase())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if symbol_import_query {
        terms.extend(named_import_binding_terms_for_query(
            &row.module,
            query,
            row.matched_symbol_name.as_deref(),
        ));
    } else {
        terms.extend(named_import_binding_terms(&row.module));
    }
    if !symbol_import_query {
        if let Some(target_symbol_names) = row.target_symbol_names.as_deref() {
            terms.extend(import_usage_identifier_terms(target_symbol_names));
        }
    }
    terms.sort();
    terms.dedup();

    terms
}

fn import_usage_context_needed(request: &CodeRetrievalRequest, rows: &[ImportRow]) -> bool {
    request.code_query_kind == CodeQueryKind::Imports
        && !rows.is_empty()
        && (target_symbol_import_query(&request.query)
            || rows.iter().any(|row| {
                row.target_symbol_names
                    .as_deref()
                    .is_some_and(|names| !names.trim().is_empty())
            }))
}

fn import_context_chunks(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &[&str],
) -> Result<Vec<(String, String)>, StorageError> {
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(paths.iter().map(|path| Value::Text((*path).to_owned())));
    values.push(Value::Integer(
        MAX_IMPORT_USAGE_CONTEXT_CHUNKS_PER_PATH as i64,
    ));
    let placeholders = placeholders(paths.len());
    let sql = format!(
        "
        SELECT path, content
        FROM (
            SELECT path, content,
                   row_number() OVER (
                       PARTITION BY path
                       ORDER BY line_start ASC, chunk_id ASC
                   ) AS path_chunk_rank
            FROM code_repository_chunks
            WHERE source_scope = ?
              AND path IN ({placeholders})
        )
        WHERE path_chunk_rank <= ?
        ORDER BY path ASC, path_chunk_rank ASC
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn import_target_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    target_paths: &[&str],
) -> Result<Vec<(String, String)>, StorageError> {
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    values.extend(
        target_paths
            .iter()
            .map(|target_path| Value::Text((*target_path).to_owned())),
    );
    let placeholders = placeholders(target_paths.len());
    let sql = format!(
        "
        SELECT path, name
        FROM code_repository_symbols
        WHERE source_scope = ?
          AND path IN ({placeholders})
          AND kind <> 'module'
        ORDER BY path ASC, line_start ASC, name ASC
        "
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

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
    let import_language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let import_generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    values.push(Value::Integer(
        candidate_limit(request, CandidateLayer::Import) as i64,
    ));
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND i.target_hint IN ({placeholders})
          {import_path_filter}
          {import_language_filter}
          {import_generated_filter}
        ORDER BY f.is_generated ASC, i.path ASC, i.line_start ASC
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
            target_symbol_names: None,
            same_file_query_usage_count: 0,
            line_range: RepositoryCodeRange {
                start: row.get(4)?,
                end: row.get(5)?,
            },
            target_hint,
            resolution_state: row.get(7)?,
            confidence_basis_points: row.get(8)?,
            confidence_tier: row.get(9)?,
            is_generated: row.get::<_, i64>(10)? != 0,
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
    let target_generated_filter = if request.exclude_generated {
        "AND NOT EXISTS (
             SELECT 1
             FROM code_repository_files target_file
             WHERE target_file.source_scope = code_repository_search.source_scope
               AND target_file.path = code_repository_search.path
               AND target_file.is_generated != 0
         )"
    } else {
        ""
    };
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
                {target_generated_filter}
              ORDER BY coalesce((
                    SELECT target_file.is_generated FROM code_repository_files target_file
                    WHERE target_file.source_scope = code_repository_search.source_scope
                      AND target_file.path = code_repository_search.path
                    LIMIT 1
                  ), 0) ASC,
                  bm25(code_repository_search) ASC,
                  record_id ASC
              LIMIT ?
        )
        ORDER BY path ASC, line_start ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(symbol_target_fts_values_for_limited(
            required_scope(status)?,
            &fts_query,
            candidate_limit(request, CandidateLayer::Symbol),
            candidate_limit(request, CandidateLayer::Symbol),
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

fn identifier_occurrences(content: &str, terms: &[String]) -> usize {
    terms
        .iter()
        .map(|term| identifier_occurrences_for_term(content, term))
        .sum()
}

fn identifier_occurrences_for_term(content: &str, term: &str) -> usize {
    let content = content.to_ascii_lowercase();
    let term = term.to_ascii_lowercase();
    content
        .match_indices(&term)
        .filter(|(index, _)| {
            identifier_match_has_boundaries(content.as_bytes(), *index, term.len())
        })
        .count()
}

fn identifier_match_has_boundaries(content: &[u8], start: usize, len: usize) -> bool {
    let before = start
        .checked_sub(1)
        .and_then(|index| content.get(index))
        .copied();
    let after = content.get(start + len).copied();

    !before.is_some_and(is_identifier_byte) && !after.is_some_and(is_identifier_byte)
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn usage_context_is_limited_to_symbol_backed_import_queries() {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        let plain = CodeRetrievalRequest::new(
            "./protocol",
            selector.clone(),
            CodeQueryKind::Imports,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        let target_symbol = CodeRetrievalRequest::new(
            "StreamEnvelope",
            selector,
            CodeQueryKind::Imports,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        assert!(!import_usage_context_needed(
            &plain,
            &[import_row("./protocol")]
        ));
        assert!(import_usage_context_needed(
            &target_symbol,
            &[import_row("./protocol")]
        ));

        let mut row = import_row("./protocol");
        row.target_symbol_names = Some("StreamEnvelope".to_owned());
        assert!(import_usage_context_needed(&plain, &[row]));
    }

    fn import_row(module: &str) -> ImportRow {
        ImportRow {
            file_id: "file".to_owned(),
            path: "src/provider.ts".to_owned(),
            language_id: "typescript".to_owned(),
            is_generated: false,
            module: module.to_owned(),
            matched_symbol_name: None,
            target_symbol_names: None,
            same_file_query_usage_count: 0,
            line_range: RepositoryCodeRange { start: 1, end: 1 },
            target_hint: None,
            resolution_state: "unresolved".to_owned(),
            confidence_basis_points: 0,
            confidence_tier: "none".to_owned(),
        }
    }
}
