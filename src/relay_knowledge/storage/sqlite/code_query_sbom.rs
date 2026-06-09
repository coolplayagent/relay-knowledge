use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest,
        RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    CandidateLayer, ScoreQuery, candidate_limit, fts_match_query, push_language_filter_values,
    push_path_filter_values, selected_row,
};
use super::{
    HitParts, code_query_rows::DependencyRow, dedupe_sort_truncate,
    fts_path_and_language_filter_sql, hit_from_parts, prepare_code_search_statement,
    required_scope,
};

pub(super) fn search_sbom(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let exclude_generated_flag = usize::from(request.exclude_generated);
    let sql = format!(
        "
        SELECT dependency.file_id, dependency.path, dependency.language_id,
               dependency.ecosystem, dependency.package_name, dependency.requirement,
               dependency.resolved_version, dependency.dependency_group,
               dependency.source_kind, dependency.is_lockfile, dependency.line_start,
               dependency.line_end, dependency.excerpt, coalesce(file.is_generated, 0)
        FROM code_repository_dependencies dependency
        LEFT JOIN code_repository_files file
          ON file.source_scope = dependency.source_scope
         AND file.path = dependency.path
        JOIN (
              SELECT source_scope, record_id, bm25(code_repository_search) AS fts_rank,
                     coalesce((SELECT file.is_generated FROM code_repository_files file WHERE file.source_scope = code_repository_search.source_scope AND file.path = code_repository_search.path LIMIT 1), 0) AS is_generated
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'dependency'
                {fts_filter}
                AND ({exclude_generated_flag} = 0 OR NOT EXISTS (SELECT 1 FROM code_repository_files file WHERE file.source_scope = code_repository_search.source_scope AND file.path = code_repository_search.path AND file.is_generated != 0))
              ORDER BY is_generated ASC, fts_rank ASC, record_id ASC
              LIMIT ?
        ) candidate
          ON candidate.source_scope = dependency.source_scope
         AND candidate.record_id = dependency.dependency_id
        WHERE dependency.source_scope = ?
        ORDER BY CASE WHEN lower(dependency.package_name) = lower(?) THEN 0 ELSE 1 END ASC,
                 candidate.is_generated ASC,
                 candidate.fts_rank ASC,
                 dependency.is_lockfile DESC,
                 dependency.path ASC,
                 dependency.line_start ASC,
                 dependency.package_name ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let source_scope = required_scope(status)?;
    let candidate_limit = candidate_limit(request, CandidateLayer::Sbom);
    let rows = statement.query_map(
        params_from_iter(sbom_query_values(
            source_scope,
            status,
            request,
            &fts_query,
            candidate_limit,
        )),
        |row| {
            Ok(DependencyRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                ecosystem: row.get(3)?,
                package_name: row.get(4)?,
                requirement: row.get(5)?,
                resolved_version: row.get(6)?,
                dependency_group: row.get(7)?,
                source_kind: row.get(8)?,
                is_lockfile: row.get(9)?,
                line_range: RepositoryCodeRange {
                    start: row.get(10)?,
                    end: row.get(11)?,
                },
                excerpt: row.get(12)?,
                is_generated: row.get::<_, i64>(13)? != 0,
            })
        },
    )?;
    let score_query = ScoreQuery::new(&request.query);
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    let mut hits = rows
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
            let score = dependency_score(&score_query, &request.query, &row);
            let excerpt = dependency_excerpt(&row);
            let edge_resolution_state = if row.is_lockfile {
                "locked".to_owned()
            } else {
                "declared".to_owned()
            };
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
                        retrieval_layers: vec![CodeRetrievalLayer::Sbom],
                        score,
                        excerpt,
                        is_generated: row.is_generated,
                        degraded_reason: None,
                        edge_kind: Some("dependency".to_owned()),
                        edge_resolution_state: Some(edge_resolution_state),
                        edge_target_hint: Some(row.package_name),
                        edge_confidence_basis_points: Some(10000),
                        edge_confidence_tier: Some("extracted".to_owned()),
                    },
                )
            })
        })
        .collect::<Vec<_>>();
    dedupe_sort_truncate(&mut hits, request.limit.max(1));

    Ok(hits)
}

fn sbom_query_values(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    fts_query: &str,
    candidate_limit: usize,
) -> Vec<Value> {
    let result_limit = request
        .limit
        .max(1)
        .saturating_mul(4)
        .min(candidate_limit.max(1));
    let mut values = vec![
        Value::Text(fts_query.to_owned()),
        Value::Text(source_scope.to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(candidate_limit as i64));
    values.push(Value::Text(source_scope.to_owned()));
    values.push(Value::Text(request.query.trim().to_owned()));
    values.push(Value::Integer(result_limit as i64));

    values
}

fn dependency_score(query: &ScoreQuery, raw_query: &str, row: &DependencyRow) -> f64 {
    let mut score = query.score([
        row.package_name.as_str(),
        row.ecosystem.as_str(),
        row.requirement.as_deref().unwrap_or_default(),
        row.resolved_version.as_deref().unwrap_or_default(),
        row.dependency_group.as_str(),
        row.source_kind.as_str(),
        row.path.as_str(),
        row.excerpt.as_str(),
    ]);
    if row.package_name.eq_ignore_ascii_case(raw_query.trim()) {
        score += 8.0;
    }
    if row.is_lockfile {
        score += 0.25;
    }
    score
}

fn dependency_excerpt(row: &DependencyRow) -> String {
    let version = row
        .resolved_version
        .as_deref()
        .or(row.requirement.as_deref())
        .unwrap_or("unversioned");
    format!(
        "{} {} {} group={} source={} {}",
        row.ecosystem,
        row.package_name,
        version,
        row.dependency_group,
        row.source_kind,
        row.excerpt
    )
}
