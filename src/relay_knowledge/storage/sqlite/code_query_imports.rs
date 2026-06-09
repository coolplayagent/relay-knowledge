use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Row, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts,
    code_query_import_scoring::{
        hybrid_import_sparse_query_penalty, import_binding_context_bonus,
        import_importer_path_context_bonus, import_line_priority,
        import_public_dependency_surface_bonus, import_reexport_surface_penalty,
        import_same_file_usage_bonus, import_self_implementation_penalty,
        import_single_module_path_tiebreaker_bonus, import_source_path_query_overlap_bonus,
        import_statement_shape_bonus, import_surface_bonus, import_target_directory_bonus,
        import_target_symbol_bonus, query_looks_like_import_path,
    },
    code_query_import_targets::{
        attach_import_query_usage_context, attach_import_target_symbols,
        search_imports_by_target_symbols, target_symbol_import_query,
    },
    code_query_path_ranking::{import_test_path_penalty, query_mentions_test_or_benchmark},
    code_query_rows::ImportRow,
    code_query_support::*,
    hit_from_parts, prepare_code_search_statement, required_scope, selected_row,
};

struct ImportPathRows {
    rows: Vec<ImportRow>,
    saturated: bool,
}

const IMPORT_PATH_DIRECT_LIMIT: usize = 200;
const MAX_IMPORT_GROUP_LOOKUP_KEYS: usize = 128;
const MAX_IMPORT_GROUP_MODULES: usize = 24;

pub(super) fn search_imports(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let direct_rows = search_import_path_rows(connection, status, request)?;
    let direct_rows_can_answer = import_path_rows_can_answer_without_fts(request, &direct_rows);
    if direct_rows_can_answer && import_path_rows_fit_request(request, &direct_rows) {
        return import_rows_to_hits(connection, status, request, direct_rows.rows);
    }

    let identifier_rows = search_import_identifier_rows(connection, status, request)?;
    let target_symbol_rows = search_imports_by_target_symbols(connection, status, request)?;
    let target_symbol_rows_can_answer =
        import_target_symbol_rows_can_answer_without_fts(request, &target_symbol_rows);
    if target_symbol_rows_can_answer && identifier_rows.is_empty() {
        return import_rows_to_hits(connection, status, request, target_symbol_rows);
    }

    match search_import_fts_rows(connection, status, request) {
        Ok(mut rows) => {
            rows.extend(direct_rows.rows);
            rows.extend(target_symbol_rows);
            rows.extend(identifier_rows);
            import_rows_to_hits(connection, status, request, rows)
        }
        Err(_) if direct_rows_can_answer => {
            import_rows_to_hits(connection, status, request, direct_rows.rows)
        }
        Err(_) if target_symbol_rows_can_answer => {
            import_rows_to_hits(connection, status, request, target_symbol_rows)
        }
        Err(error) => Err(error),
    }
}

fn search_import_identifier_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ImportRow>, StorageError> {
    let patterns = import_identifier_patterns(&request.query);
    if request.code_query_kind != CodeQueryKind::Imports || patterns.is_empty() {
        return Ok(Vec::new());
    }

    let path_filter = path_filter_sql_for_column("i.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    let predicates = patterns
        .iter()
        .map(|_| {
            "(lower(i.module) LIKE ? ESCAPE '\\' OR lower(coalesce(i.target_hint, '')) LIKE ? ESCAPE '\\')"
        })
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
        WHERE i.source_scope = ?
          AND ({predicates})
          {path_filter}
          {language_filter}
          {generated_filter}
        ORDER BY f.is_generated ASC, i.path ASC, i.line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    for pattern in patterns {
        values.push(Value::Text(pattern.clone()));
        values.push(Value::Text(pattern));
    }
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(
        candidate_limit(request, CandidateLayer::Import) as i64,
    ));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_import)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn import_identifier_patterns(query: &str) -> Vec<String> {
    query_terms(query)
        .into_iter()
        .filter(|term| term.len() >= 3)
        .filter(|term| !import_identifier_stop_term(term))
        .take(8)
        .map(|term| format!("%{}%", escape_sql_like(&term.to_ascii_lowercase())))
        .collect()
}

fn import_identifier_stop_term(term: &str) -> bool {
    matches!(
        term.to_ascii_lowercase().as_str(),
        "and"
            | "are"
            | "crate"
            | "extern"
            | "for"
            | "from"
            | "import"
            | "include"
            | "require"
            | "the"
            | "type"
            | "use"
            | "using"
    )
}

fn search_import_path_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<ImportPathRows, StorageError> {
    let Some(pattern) = import_path_lookup_pattern(request) else {
        return Ok(ImportPathRows {
            rows: Vec::new(),
            saturated: false,
        });
    };
    let direct_limit =
        candidate_limit(request, CandidateLayer::Import).min(IMPORT_PATH_DIRECT_LIMIT);
    let path_filter = path_filter_sql_for_column("i.path", status, request);
    let language_filter =
        language_filter_sql_for_columns("f.language_id", "f.path", status, request);
    let generated_filter = if request.exclude_generated {
        "AND f.is_generated = 0"
    } else {
        ""
    };
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND (
              lower(i.module) LIKE ? ESCAPE '\\'
              OR lower(coalesce(i.target_hint, '')) LIKE ? ESCAPE '\\'
          )
          {path_filter}
          {language_filter}
          {generated_filter}
        ORDER BY f.is_generated ASC, i.path ASC, i.line_start ASC
        LIMIT ?
        "
    );
    let mut values = vec![
        Value::Text(required_scope(status)?.to_owned()),
        Value::Text(pattern.clone()),
        Value::Text(pattern),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer((direct_limit + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_import)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);

    Ok(ImportPathRows { rows, saturated })
}

fn import_path_lookup_pattern(request: &CodeRetrievalRequest) -> Option<String> {
    let path_token = import_path_lookup_token(request)?;

    Some(format!(
        "%{}%",
        escape_sql_like(&path_token.to_ascii_lowercase())
    ))
}

fn import_path_lookup_token(request: &CodeRetrievalRequest) -> Option<&str> {
    if request.code_query_kind != CodeQueryKind::Imports
        || !query_looks_like_import_path(&request.query)
    {
        return None;
    }
    let path_token = request
        .query
        .split_whitespace()
        .map(import_path_token)
        .find(|token| query_looks_like_import_path(token))?;
    if path_token.is_empty() {
        return None;
    }

    Some(path_token)
}

fn import_path_token(token: &str) -> &str {
    token.trim_matches(|character: char| {
        !(character.is_ascii_alphanumeric()
            || matches!(character, '_' | '-' | '.' | '/' | '\\' | '@'))
    })
}

fn import_path_rows_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    rows: &ImportPathRows,
) -> bool {
    request.code_query_kind == CodeQueryKind::Imports
        && !rows.rows.is_empty()
        && (!rows.saturated || rows.rows.len() >= request.limit.max(1))
}

fn import_path_rows_fit_request(request: &CodeRetrievalRequest, rows: &ImportPathRows) -> bool {
    !rows.saturated && rows.rows.len() <= request.limit.max(1)
}

fn import_target_symbol_rows_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    rows: &[ImportRow],
) -> bool {
    request.code_query_kind == CodeQueryKind::Imports
        && target_symbol_import_query(&request.query)
        && !rows.is_empty()
        && rows.len() <= request.limit.max(1)
}

fn search_import_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ImportRow>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let exclude_generated_flag = usize::from(request.exclude_generated);
    let sql = format!(
        "
        SELECT i.file_id, i.path, f.language_id, i.module, i.line_start, i.line_end,
               i.target_hint, i.resolution_state, i.confidence_basis_points, i.confidence_tier,
               f.is_generated
        FROM code_repository_imports i
        INNER JOIN code_repository_files f
            ON f.source_scope = i.source_scope AND f.path = i.path
        WHERE i.source_scope = ?
          AND i.import_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'import'
                {fts_filter}
                AND ({exclude_generated_flag} = 0 OR NOT EXISTS (
                    SELECT 1 FROM code_repository_files fts_file
                    WHERE fts_file.source_scope = code_repository_search.source_scope
                      AND fts_file.path = code_repository_search.path
                      AND fts_file.is_generated != 0
                ))
              ORDER BY coalesce((
                    SELECT fts_file.is_generated FROM code_repository_files fts_file
                    WHERE fts_file.source_scope = code_repository_search.source_scope
                      AND fts_file.path = code_repository_search.path
                    LIMIT 1
                  ), 0) ASC,
                  bm25(code_repository_search) ASC,
                  record_id ASC
              LIMIT ?
          )
        ORDER BY i.path ASC, i.line_start ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request, CandidateLayer::Import),
            candidate_limit(request, CandidateLayer::Import),
        )),
        row_to_import,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn row_to_import(row: &Row<'_>) -> rusqlite::Result<ImportRow> {
    Ok(ImportRow {
        file_id: row.get(0)?,
        path: row.get(1)?,
        language_id: row.get(2)?,
        module: row.get(3)?,
        matched_symbol_name: None,
        target_symbol_names: None,
        same_file_query_usage_count: 0,
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
}

fn import_rows_to_hits(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    mut rows: Vec<ImportRow>,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if request.code_query_kind == CodeQueryKind::Imports
        && query_looks_like_import_path(&request.query)
    {
        attach_import_target_symbols(connection, status, &mut rows)?;
    }
    attach_import_query_usage_context(connection, status, request, &mut rows)?;
    let group_modules = import_group_modules(connection, status, &rows)?;

    let scoring_query = import_scoring_query(request);
    let query = scoring_query.to_lowercase();
    let score_query = ScoreQuery::new(scoring_query);
    let query_has_test_intent = query_mentions_test_or_benchmark(&request.query);

    Ok(rows
        .into_iter()
        .filter(|row| {
            selected_row(
                &row.path,
                &row.language_id,
                row.is_generated,
                status,
                request,
            )
        })
        .filter_map(|row| {
            let excerpt = import_excerpt(
                &row.module,
                row.target_symbol_names.as_deref(),
                group_modules
                    .get(&ImportGroupKey::from_row(&row))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            );
            let base_score = score_query.score([
                row.module.as_str(),
                row.target_hint.as_deref().unwrap_or_default(),
                row.matched_symbol_name.as_deref().unwrap_or_default(),
            ]) + score_exact_path(&query, &row.path)
                + scoped_identity_query_bonus(
                    scoring_query,
                    [
                        row.target_hint.as_deref().unwrap_or_default(),
                        row.matched_symbol_name.as_deref().unwrap_or_default(),
                    ],
                )
                + import_target_symbol_bonus(scoring_query, row.matched_symbol_name.as_deref());
            let score = base_score
                + import_resolution_confidence_bonus(
                    base_score,
                    &row.resolution_state,
                    row.confidence_basis_points,
                    request.code_query_kind,
                )
                + import_same_file_usage_bonus(
                    base_score,
                    row.same_file_query_usage_count,
                    request.code_query_kind,
                )
                + import_importer_path_context_bonus(
                    base_score,
                    row.same_file_query_usage_count,
                    scoring_query,
                    &row.path,
                    request.code_query_kind,
                )
                + import_target_directory_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_binding_context_bonus(
                    base_score,
                    scoring_query,
                    &row.module,
                    request.code_query_kind,
                )
                + import_statement_shape_bonus(
                    base_score,
                    &request.query,
                    &row.module,
                    request.code_query_kind,
                )
                + import_line_priority(base_score, row.line_range.start, scoring_query)
                + hybrid_import_sparse_query_penalty(
                    base_score,
                    scoring_query,
                    &row.path,
                    &row.module,
                    row.target_hint.as_deref(),
                    row.matched_symbol_name.as_deref(),
                    request.code_query_kind,
                )
                + import_public_dependency_surface_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_source_path_query_overlap_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_self_implementation_penalty(
                    base_score,
                    scoring_query,
                    &row.path,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_single_module_path_tiebreaker_bonus(
                    base_score,
                    scoring_query,
                    &row.path,
                    &row.module,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_reexport_surface_penalty(
                    base_score,
                    scoring_query,
                    &row.path,
                    &row.module,
                    row.target_hint.as_deref(),
                    request.code_query_kind,
                )
                + import_test_path_penalty(base_score, &row.path, request, query_has_test_intent)
                + import_surface_bonus(base_score, &row.path, request.code_query_kind);
            (score > 0.0).then(|| {
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
                        score: score + 1.0,
                        excerpt,
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
        })
        .collect())
}

fn import_scoring_query(request: &CodeRetrievalRequest) -> &str {
    import_path_lookup_token(request).unwrap_or(&request.query)
}

fn import_resolution_confidence_bonus(
    base_score: f64,
    resolution_state: &str,
    confidence_basis_points: u16,
    kind: CodeQueryKind,
) -> f64 {
    if base_score <= 0.0 || kind != CodeQueryKind::Imports {
        return 0.0;
    }
    match (resolution_state, confidence_basis_points) {
        ("resolved", confidence) if confidence >= 7_500 => 0.3,
        ("unresolved", confidence) if confidence <= 2_500 => -0.2,
        _ => 0.0,
    }
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct ImportGroupKey {
    path: String,
    line_start: u32,
    line_end: u32,
}

impl ImportGroupKey {
    fn from_row(row: &ImportRow) -> Self {
        Self {
            path: row.path.clone(),
            line_start: row.line_range.start,
            line_end: row.line_range.end,
        }
    }
}

fn import_group_modules(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    rows: &[ImportRow],
) -> Result<BTreeMap<ImportGroupKey, Vec<String>>, StorageError> {
    let keys = rows
        .iter()
        .map(ImportGroupKey::from_row)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(MAX_IMPORT_GROUP_LOOKUP_KEYS)
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return Ok(BTreeMap::new());
    }

    let predicates = keys
        .iter()
        .map(|_| "(path = ? AND line_start = ? AND line_end = ?)")
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "
        SELECT path, module, line_start, line_end
        FROM code_repository_imports
        WHERE source_scope = ?
          AND ({predicates})
        ORDER BY path ASC, line_start ASC, module ASC
        "
    );
    let mut values = vec![Value::Text(required_scope(status)?.to_owned())];
    for key in &keys {
        values.push(Value::Text(key.path.clone()));
        values.push(Value::Integer(i64::from(key.line_start)));
        values.push(Value::Integer(i64::from(key.line_end)));
    }

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let modules = statement.query_map(params_from_iter(values), |row| {
        Ok((
            ImportGroupKey {
                path: row.get(0)?,
                line_start: row.get(2)?,
                line_end: row.get(3)?,
            },
            row.get::<_, String>(1)?,
        ))
    })?;
    let mut groups = BTreeMap::<ImportGroupKey, Vec<String>>::new();
    for module in modules {
        let (key, module) = module.map_err(StorageError::from)?;
        let entry = groups.entry(key).or_default();
        if entry.len() < MAX_IMPORT_GROUP_MODULES && !entry.contains(&module) {
            entry.push(module);
        }
    }

    Ok(groups)
}

fn import_excerpt(
    module: &str,
    target_symbol_names: Option<&str>,
    group_modules: &[String],
) -> String {
    let mut excerpt_modules = Vec::with_capacity(group_modules.len().saturating_add(1));
    excerpt_modules.push(source_like_import_module(module));
    for group_module in group_modules {
        if group_module != module {
            let rendered = source_like_import_module(group_module);
            if !excerpt_modules.contains(&rendered) {
                excerpt_modules.push(rendered);
            }
        }
    }

    let mut excerpt = excerpt_modules.join("\n");
    if let Some(target_symbol_names) = target_symbol_names
        .map(str::trim)
        .filter(|target_symbol_names| !target_symbol_names.is_empty())
    {
        excerpt.push_str(" target symbols: ");
        excerpt.push_str(target_symbol_names);
    }

    excerpt
}

fn source_like_import_module(module: &str) -> String {
    let trimmed = module.trim();
    if trimmed.starts_with("import ")
        || trimmed.starts_with("from ")
        || trimmed.starts_with("#include")
        || trimmed.starts_with("require ")
        || trimmed.starts_with("use ")
        || trimmed.starts_with("using ")
        || trimmed.starts_with(". \"")
        || trimmed.starts_with(". '")
        || trimmed.starts_with(". $")
    {
        return trimmed.to_owned();
    }
    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    if parts.len() == 2 && import_alias_like(parts[0]) {
        return format!("{trimmed} ({} \"{}\")", parts[0], parts[1]);
    }
    if parts.len() == 1 && import_path_like(parts[0]) {
        return format!("{trimmed} (\"{trimmed}\")");
    }

    trimmed.to_owned()
}

fn import_alias_like(value: &str) -> bool {
    if matches!(value, "." | "_") {
        return true;
    }
    value
        .chars()
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && value
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn import_path_like(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '/'))
}

#[cfg(test)]
mod tests {
    use super::{import_excerpt, import_identifier_patterns, import_resolution_confidence_bonus};
    use crate::domain::CodeQueryKind;

    #[test]
    fn grouped_go_import_excerpts_include_source_like_siblings() {
        let excerpt = import_excerpt(
            "ctxalias context",
            None,
            &[
                ". strings".to_owned(),
                "_ embed".to_owned(),
                "ctxalias context".to_owned(),
            ],
        );

        assert!(excerpt.contains("ctxalias \"context\""), "{excerpt}");
        assert!(excerpt.contains(". \"strings\""), "{excerpt}");
        assert!(excerpt.contains("_ \"embed\""), "{excerpt}");
    }

    #[test]
    fn import_excerpts_keep_target_symbol_context() {
        let excerpt = import_excerpt(
            "#include \"leveldb/filter_policy.h\"",
            Some("FilterPolicy"),
            &[],
        );

        assert!(excerpt.contains("leveldb/filter_policy.h"));
        assert!(excerpt.contains("FilterPolicy"));
    }

    #[test]
    fn import_identifier_patterns_keep_alias_and_target_terms() {
        let patterns =
            import_identifier_patterns("disposeInstance as runDisposers instance registry");

        assert!(patterns.contains(&"%disposeinstance%".to_owned()));
        assert!(patterns.contains(&"%rundisposers%".to_owned()));
        assert!(patterns.contains(&"%registry%".to_owned()));
        assert!(!patterns.contains(&"%as%".to_owned()));
    }

    #[test]
    fn import_identifier_patterns_drop_import_syntax_keywords() {
        let patterns = import_identifier_patterns("import serde using System include vector");

        assert!(patterns.contains(&"%serde%".to_owned()));
        assert!(patterns.contains(&"%system%".to_owned()));
        assert!(patterns.contains(&"%vector%".to_owned()));
        assert!(!patterns.contains(&"%import%".to_owned()));
        assert!(!patterns.contains(&"%using%".to_owned()));
        assert!(!patterns.contains(&"%include%".to_owned()));
    }

    #[test]
    fn import_resolution_confidence_scores_resolved_edges_above_unresolved_edges() {
        assert!(
            import_resolution_confidence_bonus(2.0, "resolved", 8_000, CodeQueryKind::Imports)
                > 0.0
        );
        assert!(
            import_resolution_confidence_bonus(2.0, "unresolved", 2_500, CodeQueryKind::Imports)
                < 0.0
        );
        assert_eq!(
            import_resolution_confidence_bonus(2.0, "resolved", 8_000, CodeQueryKind::Hybrid),
            0.0
        );
    }
}
