use std::collections::BTreeMap;

use rusqlite::{Connection, Transaction, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest, CodeFeatureFlagUsage,
        CodeRepositoryStatus, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    SearchDocumentInserter,
    code_query_hits::{required_repository, selected_row},
};

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[CodeFeatureFlagRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_feature_flags (
            repository_id, source_scope, feature_flag_id, usage_id, file_id, path, language_id,
            name, source_kind, source_key, edge_kind, confidence_basis_points, confidence_tier,
            byte_start, byte_end, line_start, line_end, excerpt
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for record in records {
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

    Ok(())
}

pub(super) fn search(
    connection: &mut Connection,
    request: CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    super::super::retry::retry_sqlite_transient(|| {
        search_with_status(connection, &status, &request)
    })
}

fn search_with_status(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let source_scope = status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' does not have an indexed source scope",
            status.alias
        ))
    })?;
    let terms = request
        .query
        .as_deref()
        .map(query_terms)
        .unwrap_or_default();
    let retrieval_request = retrieval_like_request(request)?;
    let query = feature_flag_sql_query(source_scope, status, request, &terms);
    let mut statement = connection.prepare(&query.sql)?;
    let rows = statement.query_map(params_from_iter(query.params.iter()), |row| {
        Ok(FeatureFlagRow {
            feature_flag_id: row.get(0)?,
            usage_id: row.get(1)?,
            file_id: row.get(2)?,
            path: row.get(3)?,
            language_id: row.get(4)?,
            name: row.get(5)?,
            source_kind: row.get(6)?,
            source_key: row.get(7)?,
            edge_kind: row.get(8)?,
            confidence_basis_points: row.get(9)?,
            confidence_tier: row.get(10)?,
            byte_range: RepositoryCodeRange {
                start: row.get(11)?,
                end: row.get(12)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(13)?,
                end: row.get(14)?,
            },
            excerpt: row.get(15)?,
            related_symbol_snapshot_id: row.get(16)?,
            related_symbol_name: row.get(17)?,
        })
    })?;
    let mut groups = BTreeMap::<String, CodeFeatureFlagGraph>::new();
    for row in rows {
        let row = row?;
        if !selected_row(&row.path, &row.language_id, status, &retrieval_request) {
            continue;
        }
        if !terms.is_empty() && !row_matches_terms(&row, &terms) {
            continue;
        }
        let score = score_row(&row, &terms);
        let group = groups
            .entry(row.feature_flag_id.clone())
            .or_insert_with(|| CodeFeatureFlagGraph {
                feature_flag_id: row.feature_flag_id.clone(),
                name: row.name.clone(),
                source_kind: row.source_kind.clone(),
                source_key: row.source_key.clone(),
                score,
                usages: Vec::new(),
            });
        group.score = group.score.max(score);
        group.usages.push(CodeFeatureFlagUsage {
            usage_id: row.usage_id,
            path: row.path,
            language_id: row.language_id,
            file_id: row.file_id,
            byte_range: row.byte_range,
            line_range: row.line_range,
            edge_kind: row.edge_kind,
            related_symbol_snapshot_id: row.related_symbol_snapshot_id,
            related_symbol_name: row.related_symbol_name,
            confidence_basis_points: row.confidence_basis_points,
            confidence_tier: row.confidence_tier,
            excerpt: row.excerpt,
        });
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    for group in &mut groups {
        group.usages.sort_by(|left, right| {
            edge_priority(&left.edge_kind)
                .cmp(&edge_priority(&right.edge_kind))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.line_range.start.cmp(&right.line_range.start))
        });
    }
    groups.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.source_key.cmp(&right.source_key))
    });
    groups.truncate(request.limit);

    Ok(groups)
}

fn feature_flag_sql_query(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeFeatureFlagRequest,
    terms: &[String],
) -> FeatureFlagSqlQuery {
    let FeatureFlagSqlFilter {
        where_clause,
        params: filter_params,
    } = feature_flag_sql_filter(source_scope, status, request, terms);
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
               flag.byte_start, flag.byte_end, flag.line_start, flag.line_end, flag.excerpt,
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
        WHERE {where_clause}
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
    params.push(Value::Integer(request.limit as i64));
    params.extend(filter_params);

    FeatureFlagSqlQuery { sql, params }
}

#[derive(Debug)]
struct FeatureFlagRow {
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
        "lower(flag.name) LIKE ? ESCAPE '\\'",
        "lower(flag.source_kind) LIKE ? ESCAPE '\\'",
        "lower(flag.source_key) LIKE ? ESCAPE '\\'",
        "lower(flag.edge_kind) LIKE ? ESCAPE '\\'",
        "lower(flag.path) LIKE ? ESCAPE '\\'",
        "lower(flag.excerpt) LIKE ? ESCAPE '\\'",
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

fn retrieval_like_request(
    request: &CodeFeatureFlagRequest,
) -> Result<crate::domain::CodeRetrievalRequest, StorageError> {
    crate::domain::CodeRetrievalRequest::new(
        request.query.clone().unwrap_or_else(|| "*".to_owned()),
        request.repository.clone(),
        crate::domain::CodeQueryKind::Hybrid,
        request.limit.clamp(1, 50),
        request.freshness_policy,
    )
    .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
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
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn feature_flag_sql_applies_filters_and_limit_before_usage_lookup() {
        let selector = CodeRepositorySelector::new(
            "fixture",
            "commit",
            vec!["./src/payments".to_owned()],
            vec!["rust".to_owned()],
        )
        .expect("selector should validate");
        let request = CodeFeatureFlagRequest::new(
            Some("CHECKOUT_V2".to_owned()),
            selector,
            1,
            FreshnessPolicy::AllowStale,
        )
        .expect("feature flag request should validate");
        let terms = request
            .query
            .as_deref()
            .map(query_terms)
            .unwrap_or_default();

        let query = feature_flag_sql_query("scope", &status(), &request, &terms);

        assert!(query.sql.contains("WITH filtered_flags AS"));
        assert!(query.sql.contains("LIMIT ?"));
        assert_eq!(query.sql.matches("flag.source_scope = ?").count(), 2);
        assert_eq!(
            query
                .sql
                .matches("flag.path = ? OR flag.path LIKE ? ESCAPE '\\'")
                .count(),
            4
        );
        assert_eq!(query.sql.matches("flag.language_id IN").count(), 4);
        assert!(query.sql.contains("lower(flag.source_key) LIKE ?"));
        assert_eq!(query.params.len(), 27);
        assert!(query.params.contains(&Value::Integer(1)));
        assert!(
            query
                .params
                .contains(&Value::Text("src/payments/%".to_owned()))
        );
        assert!(
            query
                .params
                .contains(&Value::Text("%checkout\\_v2%".to_owned()))
        );
    }

    fn status() -> CodeRepositoryStatus {
        CodeRepositoryStatus {
            repository_id: "repo".to_owned(),
            alias: "fixture".to_owned(),
            root_path: "/tmp/repo".to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            last_indexed_scope_id: Some("scope".to_owned()),
            last_indexed_commit: Some("commit".to_owned()),
            tree_hash: Some("tree".to_owned()),
            state: "indexed".to_owned(),
            indexed_file_count: 1,
            symbol_count: 0,
            reference_count: 0,
            chunk_count: 0,
            stale: false,
            degraded_reason: None,
        }
    }
}
