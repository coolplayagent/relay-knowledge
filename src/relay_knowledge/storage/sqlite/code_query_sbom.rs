use rusqlite::{Connection, params_from_iter};

use crate::{
    domain::{
        CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer, CodeRetrievalRequest,
        RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{CandidateLayer, ScoreQuery, candidate_limit, fts_match_query, selected_row};
use super::{
    HitParts, code_query_rows::DependencyRow, fts_path_and_language_filter_sql,
    fts_values_for_limited_with_language, hit_from_parts, prepare_code_search_statement,
    required_scope,
};

pub(super) fn search_sbom(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = format!(
        "
        SELECT dependency.file_id, dependency.path, dependency.language_id,
               dependency.ecosystem, dependency.package_name, dependency.requirement,
               dependency.resolved_version, dependency.dependency_group,
               dependency.source_kind, dependency.is_lockfile, dependency.line_start,
               dependency.line_end, dependency.excerpt
        FROM code_repository_dependencies dependency
        WHERE dependency.source_scope = ?
          AND dependency.dependency_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'dependency'
                {fts_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        ORDER BY dependency.path ASC, dependency.line_start ASC, dependency.package_name ASC
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
            candidate_limit(request, CandidateLayer::Sbom),
            candidate_limit(request, CandidateLayer::Sbom),
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
            })
        },
    )?;
    let score_query = ScoreQuery::new(&request.query);
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;

    Ok(rows
        .into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
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
        .collect())
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
