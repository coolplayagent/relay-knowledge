//! Feature-flag graph persistence, filtering, and ranked query ownership.
mod registry;

use rusqlite::{Connection, Transaction, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest, CodeFeatureFlagUsage,
        CodeRepositoryStatus, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{SearchDocumentInserter, query::hits::required_repository};

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[CodeFeatureFlagRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_feature_flags (
            repository_id, source_scope, feature_flag_id, usage_id, file_id, path, language_id,
            name, source_kind, source_key, edge_kind, confidence_basis_points, confidence_tier,
            byte_start, byte_end, line_start, line_end, excerpt, metadata_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for record in records {
        let metadata = serde_json::to_string(&record.metadata)
            .map_err(|e| StorageError::InvalidInput(e.to_string()))?;
        if metadata.len() > 65_536 {
            return Err(StorageError::InvalidInput(
                "configuration metadata exceeds 64 KiB".into(),
            ));
        }
        statement.execute(params![
            record.repository_id,
            record.source_scope,
            record.feature_flag_id,
            record.usage_id,
            record.file_id,
            record.path,
            record.language_id,
            record.name,
            record.source_kind,
            record.source_key,
            record.edge_kind,
            record.confidence_basis_points,
            record.confidence_tier,
            record.byte_range.start,
            record.byte_range.end,
            record.line_range.start,
            record.line_range.end,
            record.excerpt,
            metadata,
        ])?;
        search_documents.insert(
            &record.source_scope,
            "feature_flag",
            &record.usage_id,
            &record.path,
            &record.language_id,
            [
                record.name.as_str(),
                record.source_kind.as_str(),
                record.source_key.as_str(),
                record.edge_kind.as_str(),
                record.excerpt.as_str(),
                record.path.as_str(),
            ],
        )?;
    }
    search_documents.finish()?;

    Ok(())
}

pub(super) fn search(
    connection: &mut Connection,
    request: CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        search_with_status(connection, &status, &request)
    })
}

pub(super) fn search_scope(
    connection: &mut Connection,
    source_scope: &str,
    request: CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let status = super::status::repository_scope_status_by_source_scope(connection, source_scope)?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository source scope '{source_scope}' is not indexed"
            ))
        })?;
    super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        search_with_status(connection, &status, &request)
    })
}

fn search_with_status(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    registry::search(connection, status, request)
}

fn feature_flag_sql_query(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
    terms: &[String],
) -> FeatureFlagSqlQuery {
    let FeatureFlagSqlFilter {
        mut where_clause,
        params: mut filter_params,
    } = feature_flag_sql_filter(source_scope, status, request, terms);
    for (field, value) in [
        ("domain", request.filters.domain.clone().map(Value::Text)),
        (
            "source_format",
            request.filters.source.clone().map(Value::Text),
        ),
        (
            "hot_reload",
            request
                .filters
                .hot_reload
                .map(|value| Value::Integer(i64::from(value))),
        ),
    ] {
        if let Some(value) = value {
            let symbolic = if terms.is_empty() {
                ""
            } else {
                "flag.source_kind='config_symbol' OR "
            };
            where_clause.push_str(&format!(" AND ({symbolic}EXISTS (SELECT 1 FROM code_repository_feature_flags metadata_flag WHERE metadata_flag.source_scope=flag.source_scope AND metadata_flag.feature_flag_id=flag.feature_flag_id AND json_extract(metadata_flag.metadata_json,'$.{field}') = ?))"));
            filter_params.push(value);
        }
    }
    where_clause.push_str(" AND flag.edge_kind != 'declares_config_getter'");
    if terms.is_empty() {
        where_clause.push_str(" AND (flag.source_kind != 'config_symbol' OR json_extract(flag.metadata_json,'$.target_kind') IS NOT NULL)");
    }
    where_clause.push_str(" AND (flag.edge_kind != 'declares_string_constant' OR EXISTS (SELECT 1 FROM code_repository_feature_flags evidence, json_each(flag.metadata_json,'$.bindings') binding WHERE evidence.source_scope=flag.source_scope AND json_extract(evidence.metadata_json,'$.target_kind') IS NOT NULL AND json_extract(evidence.metadata_json,'$.reference')=binding.value))");
    let usage_filter = feature_flag_sql_filter(source_scope, status, request, &[]);
    let usage_where = &usage_filter.where_clause;
    let query_bonus = if terms.is_empty() { "0.0" } else { "8.0" };
    let sql = format!(
        "
        WITH filtered_flags AS (
            SELECT flag.feature_flag_id,
                   MAX(
                       CASE flag.edge_kind
                         WHEN 'guards_code' THEN 20.0
                         WHEN 'defines_config' THEN 16.0
                         ELSE 12.0
                       END + CAST(flag.confidence_basis_points AS REAL) / 1000.0 + {query_bonus}
                   ) AS rank_score,
                   MIN(flag.name) AS sort_name,
                   MIN(flag.source_key) AS sort_source_key
            FROM code_repository_feature_flags flag
            WHERE {where_clause}
            GROUP BY flag.feature_flag_id
            ORDER BY rank_score DESC, sort_name ASC, sort_source_key ASC
            LIMIT ?
        )
        SELECT flag.feature_flag_id, flag.usage_id, flag.file_id, flag.path, flag.language_id,
               flag.name, flag.source_kind, flag.source_key, flag.edge_kind,
               flag.confidence_basis_points, flag.confidence_tier,
               flag.byte_start, flag.byte_end, flag.line_start, flag.line_end, flag.excerpt, flag.metadata_json,
               (
                   SELECT symbol_snapshot_id
                   FROM code_repository_symbols symbol
                   WHERE symbol.source_scope = flag.source_scope
                     AND symbol.path = flag.path
                     AND symbol.line_start <= flag.line_start
                     AND symbol.line_end >= flag.line_start
                   ORDER BY symbol.line_start DESC, symbol.line_end ASC
                   LIMIT 1
               ) AS related_symbol_snapshot_id,
               (
                   SELECT name
                   FROM code_repository_symbols symbol
                   WHERE symbol.source_scope = flag.source_scope
                     AND symbol.path = flag.path
                     AND symbol.line_start <= flag.line_start
                     AND symbol.line_end >= flag.line_start
                   ORDER BY symbol.line_start DESC, symbol.line_end ASC
                   LIMIT 1
               ) AS related_symbol_name
        FROM code_repository_feature_flags flag
        JOIN filtered_flags selected ON selected.feature_flag_id = flag.feature_flag_id
        WHERE {usage_where}
        ORDER BY flag.name ASC,
                 CASE flag.edge_kind
                   WHEN 'guards_code' THEN 0
                   WHEN 'defines_config' THEN 1
                   ELSE 2
                 END,
                 flag.path ASC,
                 flag.line_start ASC
        "
    );
    let mut params = filter_params.clone();
    params.push(Value::Integer(registry::MAX_ROWS as i64 + 1));
    params.extend(usage_filter.params);

    FeatureFlagSqlQuery { sql, params }
}

#[derive(Debug, Clone)]
struct FeatureFlagRow {
    metadata: crate::domain::CodeConfigMetadata,
    feature_flag_id: String,
    usage_id: String,
    file_id: String,
    path: String,
    language_id: String,
    name: String,
    source_kind: String,
    source_key: String,
    edge_kind: String,
    confidence_basis_points: u16,
    confidence_tier: String,
    byte_range: RepositoryCodeRange,
    line_range: RepositoryCodeRange,
    excerpt: String,
    related_symbol_snapshot_id: Option<String>,
    related_symbol_name: Option<String>,
}

struct FeatureFlagSqlQuery {
    sql: String,
    params: Vec<Value>,
}

struct FeatureFlagSqlFilter {
    where_clause: String,
    params: Vec<Value>,
}

fn feature_flag_sql_filter(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
    terms: &[String],
) -> FeatureFlagSqlFilter {
    let mut clauses = vec!["flag.source_scope = ?".to_owned()];
    let mut params = vec![Value::Text(source_scope.to_owned())];
    append_path_filter_clause(&mut clauses, &mut params, &status.path_filters);
    append_path_filter_clause(&mut clauses, &mut params, &request.repository.path_filters);
    append_language_filter_clause(&mut clauses, &mut params, &status.language_filters);
    append_language_filter_clause(
        &mut clauses,
        &mut params,
        &request.repository.language_filters,
    );
    append_query_term_clauses(&mut clauses, &mut params, terms);

    FeatureFlagSqlFilter {
        where_clause: clauses.join(" AND "),
        params,
    }
}

fn append_path_filter_clause(
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
    filters: &[String],
) {
    if filters.is_empty() {
        return;
    }

    let mut fragments = Vec::new();
    for filter in filters {
        let filter = normalize_sql_path_filter(filter);
        if filter == "." {
            return;
        }
        if filter.is_empty() {
            continue;
        }
        fragments.push("(flag.path = ? OR flag.path LIKE ? ESCAPE '\\')".to_owned());
        params.push(Value::Text(filter.to_owned()));
        params.push(Value::Text(format!("{}/%", escape_like_pattern(filter))));
    }

    if fragments.is_empty() {
        clauses.push("0 = 1".to_owned());
    } else {
        clauses.push(format!("({})", fragments.join(" OR ")));
    }
}

fn append_language_filter_clause(
    clauses: &mut Vec<String>,
    params: &mut Vec<Value>,
    filters: &[String],
) {
    if filters.is_empty() {
        return;
    }

    let mut unique = Vec::<&str>::new();
    for filter in filters {
        if !filter.is_empty() && !unique.contains(&filter.as_str()) {
            unique.push(filter);
        }
    }
    if unique.is_empty() {
        clauses.push("0 = 1".to_owned());
        return;
    }

    clauses.push(format!(
        "flag.language_id IN ({})",
        vec!["?"; unique.len()].join(", ")
    ));
    for filter in unique {
        params.push(Value::Text(filter.to_owned()));
    }
}

fn append_query_term_clauses(clauses: &mut Vec<String>, params: &mut Vec<Value>, terms: &[String]) {
    let fields = [
        "config_casefold(flag.name) LIKE ? ESCAPE '\\'",
        "config_casefold(flag.source_kind) LIKE ? ESCAPE '\\'",
        "config_casefold(flag.source_key) LIKE ? ESCAPE '\\'",
        "config_casefold(flag.edge_kind) LIKE ? ESCAPE '\\'",
        "config_casefold(flag.path) LIKE ? ESCAPE '\\'",
        "config_casefold(flag.excerpt) LIKE ? ESCAPE '\\'",
        "config_casefold(flag.metadata_json) LIKE ? ESCAPE '\\'",
    ];
    for term in terms {
        clauses.push(format!("({})", fields.join(" OR ")));
        let pattern = format!("%{}%", escape_like_pattern(term));
        for _ in fields {
            params.push(Value::Text(pattern.clone()));
        }
    }
}

fn normalize_sql_path_filter(filter: &str) -> &str {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    filter
}

fn escape_like_pattern(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }

    escaped
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn row_matches_terms(row: &FeatureFlagRow, terms: &[String]) -> bool {
    let haystack = format!(
        "{} {} {} {} {} {}",
        row.name, row.source_kind, row.source_key, row.edge_kind, row.path, row.excerpt
    )
    .to_ascii_lowercase();
    terms.iter().all(|term| haystack.contains(term))
}

fn score_row(row: &FeatureFlagRow, terms: &[String]) -> f64 {
    let edge_score = match row.edge_kind.as_str() {
        "guards_code" => 20.0,
        "defines_config" => 16.0,
        _ => 12.0,
    };
    let confidence = f64::from(row.confidence_basis_points) / 1000.0;
    let query_bonus = if terms.is_empty() {
        0.0
    } else if row_matches_terms(row, terms) {
        8.0
    } else {
        0.0
    };

    edge_score + confidence + query_bonus
}

fn edge_priority(edge_kind: &str) -> usize {
    match edge_kind {
        "guards_code" => 0,
        "defines_config" => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod mod_tests;
