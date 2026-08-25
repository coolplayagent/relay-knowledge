use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use rusqlite::{Connection, ErrorCode, params_from_iter, types::Value};

use crate::storage::sqlite::code::search::EXACT_SEARCH_OWNER_PREDICATE_SQL;
use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalRequest, RepositoryCodeRange},
    storage::StorageError,
};

use super::super::relevance::{
    CandidateLayer, candidate_limit, language_filter_sql_for_columns, path_filter_sql_for_column,
    push_language_filter_values, push_path_filter_values, symbol_fts_match_query,
};
use super::super::rows::ImportRow;
use super::super::{
    code_search_read_model_unavailable_reason,
    identifiers::{code_outside_comments_and_literals, identifier_terms_equivalent},
    prepare_code_search_statement, required_scope,
};
use super::binding_terms::{
    import_surface_declares_local_binding, import_usage_identifier_terms,
    named_import_binding_terms, named_import_binding_terms_for_query, query_local_binding_terms,
    terminal_import_binding_terms,
};
use super::path_context::{
    import_path_lookup_token, import_path_token_matches_target_hint, import_target_symbol_query,
    query_looks_like_import_path,
};

const SQLITE_BIND_BATCH_SIZE: usize = 500;
const MAX_TARGET_SYMBOL_NAMES_PER_IMPORT: usize = 4;
const MAX_IMPORT_USAGE_CONTEXT_CHUNKS_PER_PATH: usize = 2;
const MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL: usize = 2_048;
const MAX_IMPORT_USAGE_CONTEXT_TERMS: usize = 128;
const MAX_IMPORT_USAGE_CONTEXT_TERM_BYTES: usize = 8 * 1_024;
const MAX_IMPORT_USAGE_CONTEXT_PATHS: usize = super::IMPORT_EXACT_EDGE_RESERVE_LIMIT;
const MAX_IMPORT_USAGE_CONTEXT_BYTES_TOTAL: usize =
    MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL * 8 * 1_024;
const IMPORT_CONTEXT_PATH_BIND_BATCH_SIZE: usize = SQLITE_BIND_BATCH_SIZE - 4;
const IMPORT_CONTEXT_SQL_PROGRESS_INTERVAL: i32 = 1_000;
const MAX_IMPORT_CONTEXT_SQL_PROGRESS_CALLBACKS: usize = 4_096;

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
    if request.code_query_kind != CodeQueryKind::Imports || rows.is_empty() {
        return Ok(());
    }
    let usage_terms_by_row = rows
        .iter()
        .map(|row| import_usage_context_terms_for_row(&request.query, row))
        .collect::<Vec<_>>();
    if usage_terms_by_row.iter().all(Vec::is_empty) {
        return Ok(());
    }
    let Some(paths) = eligible_import_usage_context_paths(rows, &usage_terms_by_row) else {
        return Ok(());
    };
    let Some(fts_query) = import_usage_context_fts_query(&usage_terms_by_row) else {
        return Ok(());
    };
    let language_by_path = rows
        .iter()
        .map(|row| (row.path.clone(), row.language_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut content_by_path = BTreeMap::<String, Vec<String>>::new();
    let mut remaining_context_chunks = MAX_IMPORT_USAGE_CONTEXT_CHUNKS_TOTAL;
    let mut remaining_context_bytes = MAX_IMPORT_USAGE_CONTEXT_BYTES_TOTAL;
    let mut context_saturated = false;
    for path_chunk in paths.chunks(IMPORT_CONTEXT_PATH_BIND_BATCH_SIZE) {
        let context_probe = match import_context_chunks(
            connection,
            status,
            path_chunk,
            &fts_query,
            remaining_context_chunks,
            remaining_context_bytes,
        ) {
            Ok(context_chunks) => context_chunks,
            Err(error) if code_search_read_model_unavailable_reason(&error).is_some() => {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if context_probe.saturated {
            context_saturated = true;
            break;
        }
        let context_chunks = context_probe.chunks;
        remaining_context_chunks -= context_chunks.len();
        remaining_context_bytes -= context_probe.byte_len;
        for (path, content) in context_chunks {
            let language_id = language_by_path
                .get(&path)
                .map(String::as_str)
                .unwrap_or_default();
            content_by_path
                .entry(path)
                .or_default()
                .push(code_outside_comments_and_literals(language_id, &content));
        }
    }
    if context_saturated {
        content_by_path.clear();
    }

    for (row, usage_terms) in rows.iter_mut().zip(&usage_terms_by_row) {
        if usage_terms.is_empty() {
            continue;
        }
        let usage = content_by_path
            .get(&row.path)
            .map(|contents| {
                contents.iter().fold(0usize, |usage, content| {
                    usage.saturating_add(identifier_occurrences(content, usage_terms))
                })
            })
            .unwrap_or_default();
        let import_line_usage = identifier_occurrences(&row.module, usage_terms);
        row.same_file_query_usage_count = usage.saturating_sub(import_line_usage);
    }

    Ok(())
}

fn eligible_import_usage_context_paths<'row>(
    rows: &'row [ImportRow],
    usage_terms_by_row: &[Vec<String>],
) -> Option<Vec<&'row str>> {
    let paths = rows
        .iter()
        .zip(usage_terms_by_row)
        .filter(|(_, terms)| !terms.is_empty())
        .map(|(row, _)| row.path.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    (paths.len() <= MAX_IMPORT_USAGE_CONTEXT_PATHS).then_some(paths)
}

fn import_usage_context_fts_query(usage_terms_by_row: &[Vec<String>]) -> Option<String> {
    let mut terms = BTreeSet::new();
    let mut term_bytes = 0usize;
    for term in usage_terms_by_row.iter().flatten() {
        if terms.contains(term) {
            continue;
        }
        let next_bytes = term_bytes.saturating_add(term.len());
        if terms.len() >= MAX_IMPORT_USAGE_CONTEXT_TERMS
            || next_bytes > MAX_IMPORT_USAGE_CONTEXT_TERM_BYTES
        {
            return None;
        }
        term_bytes = next_bytes;
        terms.insert(term.clone());
    }
    if terms.is_empty() {
        return None;
    }

    Some(symbol_fts_match_query(
        &terms.into_iter().collect::<Vec<_>>().join(" "),
    ))
}

fn import_usage_terms_for_row(query: &str, row: &ImportRow) -> Vec<String> {
    let symbol_import_query = import_target_symbol_query(query).is_some();
    let query_binding_terms = query_local_binding_terms(query);
    let matched_symbol_binding_terms = row
        .matched_symbol_name
        .as_deref()
        .map(|names| matched_symbol_query_binding_terms(names, &query_binding_terms))
        .unwrap_or_default();
    let matched_symbol_bindings = matched_symbol_binding_terms.join(" ");
    let mut terms = Vec::new();
    if symbol_import_query && !matched_symbol_binding_terms.is_empty() {
        terms.extend(
            matched_symbol_binding_terms
                .iter()
                .flat_map(|term| import_usage_identifier_terms(term)),
        );
    } else if symbol_import_query {
        terms.extend(named_import_binding_terms_for_query(
            &row.module,
            query,
            (!matched_symbol_bindings.is_empty()).then_some(matched_symbol_bindings.as_str()),
        ));
    } else {
        terms.extend(named_import_binding_terms(&row.module));
    }
    if !symbol_import_query {
        if let Some(target_symbol_names) = row.target_symbol_names.as_deref() {
            terms.extend(import_usage_identifier_terms(target_symbol_names));
        }
    }
    let terminal_terms = terminal_import_binding_terms(&row.module);
    if !symbol_import_query
        || (matched_symbol_binding_terms.is_empty()
            && import_surface_mentions_query_binding(&row.module, &query_binding_terms))
    {
        terms.extend(terminal_terms);
    }
    if symbol_import_query && terms.is_empty() && !matched_symbol_bindings.is_empty() {
        terms.extend(import_usage_identifier_terms(&matched_symbol_bindings));
    }
    terms.sort();
    terms.dedup();

    terms
}

fn matched_symbol_query_binding_terms(
    matched_symbol_names: &str,
    query_binding_terms: &[String],
) -> Vec<String> {
    query_identifier_terms(matched_symbol_names)
        .into_iter()
        .filter(|term| identifier_term_mentions_query_binding(term, query_binding_terms))
        .collect()
}

fn identifier_term_mentions_query_binding(term: &str, query_binding_terms: &[String]) -> bool {
    let normalized = term.to_ascii_lowercase();
    query_binding_terms.contains(&normalized)
        || import_usage_identifier_terms(term)
            .iter()
            .any(|term| query_binding_terms.contains(term))
}

fn import_surface_mentions_query_binding(module: &str, query_terms: &[String]) -> bool {
    query_identifier_terms(module)
        .iter()
        .any(|term| identifier_term_mentions_query_binding(term, query_terms))
}

fn import_usage_context_terms_for_row(query: &str, row: &ImportRow) -> Vec<String> {
    let query_binding_terms = query_local_binding_terms(query);
    let matched_symbol_is_query_local = row.matched_symbol_name.as_deref().is_some_and(|names| {
        !matched_symbol_query_binding_terms(names, &query_binding_terms).is_empty()
    });
    let target_symbol_is_query_local = row.target_symbol_names.as_deref().is_some_and(|names| {
        !matched_symbol_query_binding_terms(names, &query_binding_terms).is_empty()
    });
    let target_symbol_evidence = target_symbol_is_query_local
        || (query_looks_like_import_path(query)
            && row
                .target_symbol_names
                .as_deref()
                .is_some_and(|names| !names.trim().is_empty()));
    if !import_surface_declares_local_binding(&row.module)
        && !matched_symbol_is_query_local
        && !target_symbol_evidence
    {
        return Vec::new();
    }
    if !import_surface_mentions_query_binding(&row.module, &query_binding_terms)
        && !matched_symbol_is_query_local
        && !target_symbol_evidence
    {
        return Vec::new();
    }
    let terms = import_usage_terms_for_row(query, row);
    terms
        .iter()
        .filter(|term| import_usage_term_is_local_binding(term, &terms))
        .cloned()
        .collect()
}

fn import_usage_term_is_local_binding(term: &str, terms: &[String]) -> bool {
    let normalized = normalized_usage_term(term);
    !normalized.is_empty()
        && !terms.iter().any(|other| {
            let other_normalized = normalized_usage_term(other);
            other_normalized.len() > normalized.len()
                && other_normalized.contains(&normalized)
                && other_normalized.strip_suffix('s') != Some(normalized.as_str())
        })
}

fn normalized_usage_term(term: &str) -> String {
    term.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

fn import_context_chunks(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &[&str],
    fts_query: &str,
    max_chunks: usize,
    max_bytes: usize,
) -> Result<ImportContextProbe, StorageError> {
    import_context_chunks_with_progress_budget(
        connection,
        status,
        paths,
        fts_query,
        ImportContextProbeBudget {
            max_chunks,
            max_bytes,
            progress_interval: IMPORT_CONTEXT_SQL_PROGRESS_INTERVAL,
            max_progress_callbacks: MAX_IMPORT_CONTEXT_SQL_PROGRESS_CALLBACKS,
        },
    )
}

struct ImportContextProbe {
    chunks: Vec<(String, String)>,
    byte_len: usize,
    saturated: bool,
}

struct ImportContextProbeBudget {
    max_chunks: usize,
    max_bytes: usize,
    progress_interval: i32,
    max_progress_callbacks: usize,
}

fn import_context_chunks_with_progress_budget(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &[&str],
    fts_query: &str,
    budget: ImportContextProbeBudget,
) -> Result<ImportContextProbe, StorageError> {
    let progress_callbacks = Arc::new(AtomicUsize::new(0));
    let observed_callbacks = Arc::clone(&progress_callbacks);
    connection.progress_handler(
        budget.progress_interval,
        Some(move || {
            observed_callbacks.fetch_add(1, Ordering::Relaxed) >= budget.max_progress_callbacks
        }),
    );
    let result = import_context_chunks_with_active_progress_handler(
        connection,
        status,
        paths,
        fts_query,
        budget.max_chunks,
        budget.max_bytes,
    );
    connection.progress_handler(0, None::<fn() -> bool>);

    match result {
        Err(StorageError::Sqlite(error)) if sqlite_operation_interrupted(&error) => {
            Ok(ImportContextProbe {
                chunks: Vec::new(),
                byte_len: 0,
                saturated: true,
            })
        }
        other => other,
    }
}

fn import_context_chunks_with_active_progress_handler(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    paths: &[&str],
    fts_query: &str,
    max_chunks: usize,
    max_bytes: usize,
) -> Result<ImportContextProbe, StorageError> {
    let mut values = vec![
        Value::Text(fts_query.to_owned()),
        Value::Text(required_scope(status)?.to_owned()),
    ];
    values.extend(paths.iter().map(|path| Value::Text((*path).to_owned())));
    values.push(Value::Text(required_scope(status)?.to_owned()));
    let fair_sample_cap = paths
        .len()
        .saturating_mul(MAX_IMPORT_USAGE_CONTEXT_CHUNKS_PER_PATH)
        .min(max_chunks);
    values.push(Value::Integer((fair_sample_cap + 1) as i64));
    let placeholders = placeholders(paths.len());
    let sql = format!(
        "
        WITH path_samples AS (
            SELECT code_repository_search.path,
                   MIN(code_repository_search.record_id) AS first_record_id,
                   MAX(code_repository_search.record_id) AS last_record_id
            FROM code_repository_search
            WHERE code_repository_search MATCH ?
              AND code_repository_search.source_scope = ?
              AND code_repository_search.document_kind = 'chunk'
              {EXACT_SEARCH_OWNER_PREDICATE_SQL}
              AND code_repository_search.path IN ({placeholders})
            GROUP BY code_repository_search.path
        )
        SELECT path_samples.path, chunk.content
        FROM path_samples
        INNER JOIN code_repository_chunks chunk
            ON chunk.source_scope = ?
           AND chunk.path = path_samples.path
           AND chunk.chunk_id IN (
               path_samples.first_record_id,
               path_samples.last_record_id
           )
        ORDER BY path_samples.path ASC, chunk.chunk_id ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut chunks = Vec::new();
    let mut byte_len = 0usize;
    let mut observed_rows = 0usize;
    for row in rows {
        let (path, content) = row.map_err(StorageError::from)?;
        observed_rows = observed_rows.saturating_add(1);
        if observed_rows > fair_sample_cap {
            return Ok(ImportContextProbe {
                chunks: Vec::new(),
                byte_len: 0,
                saturated: true,
            });
        }
        if content.len() > max_bytes.saturating_sub(byte_len) {
            return Ok(ImportContextProbe {
                chunks: Vec::new(),
                byte_len: 0,
                saturated: true,
            });
        }
        byte_len += content.len();
        chunks.push((path, content));
    }
    Ok(ImportContextProbe {
        chunks,
        byte_len,
        saturated: false,
    })
}

fn sqlite_operation_interrupted(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == ErrorCode::OperationInterrupted
    )
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
    if request.code_query_kind != CodeQueryKind::Imports {
        return Ok(Vec::new());
    }
    let Some(symbol_query) = import_target_symbol_query(&request.query) else {
        return Ok(Vec::new());
    };
    let import_path = import_path_lookup_token(&request.query)
        .filter(|path_token| !super::path_context::query_contains_file_extension(path_token));
    let symbol_targets =
        import_target_symbol_matches(connection, status, request, symbol_query, import_path)?;
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
        let remaining = super::IMPORT_EXACT_EDGE_RESERVE_LIMIT.saturating_sub(rows.len());
        if remaining == 0 {
            break;
        }
        rows.extend(search_imports_by_target_hint_chunk(
            connection,
            status,
            request,
            target_hint_chunk,
            &matched_names_by_hint,
            remaining,
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
    limit: usize,
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
    values.push(Value::Integer(limit as i64));
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated, f.line_count
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
            source_line_count: row.get(11)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn import_target_symbol_matches(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    symbol_query: &str,
    import_path: Option<&str>,
) -> Result<Vec<ImportTargetSymbol>, StorageError> {
    let fts_query = symbol_fts_match_query(symbol_query);
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
                {EXACT_SEARCH_OWNER_PREDICATE_SQL}
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
    let mut targets = Vec::new();
    for row in rows {
        let (path, name, language_id) = row?;
        if !symbol_matches_import_target_query(symbol_query, &name) {
            continue;
        }
        for target_hint in import_target_hints_for_symbol(&path, &language_id) {
            if import_path.is_some_and(|path_token| {
                !import_path_token_matches_target_hint(path_token, &target_hint)
            }) {
                continue;
            }
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

fn symbol_matches_import_target_query(query: &str, name: &str) -> bool {
    query_identifier_terms(query)
        .last()
        .is_some_and(|term| identifier_terms_equivalent(name, term))
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
#[path = "targets_tests.rs"]
mod tests;
