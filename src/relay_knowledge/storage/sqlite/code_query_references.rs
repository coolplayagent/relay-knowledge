use rusqlite::{Connection, Row, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::{
    HitParts, code_query_rows::ReferenceRow, code_query_support::*, dedupe_sort_truncate,
    hit_from_parts, prepare_code_search_statement, required_scope, selected_row,
};

struct ReferenceIdentityRows {
    rows: Vec<ReferenceRow>,
    saturated: bool,
}

pub(super) fn search_references(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let identity = SymbolIdentityQuery::from_query(&request.query);
    let mut identity_hits = Vec::new();
    if let Some(identity) = &identity {
        let identity_rows = search_reference_identity_rows(connection, status, request, identity)?;
        let saturated = identity_rows.saturated;
        let rows = identity_rows
            .rows
            .into_iter()
            .filter(|row| {
                identity.matches_symbol(
                    &row.name,
                    "",
                    row.target_hint.as_deref().unwrap_or_default(),
                    row.target_canonical_symbol_id
                        .as_deref()
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        identity_hits = reference_rows_to_hits(status, request, rows);
        if reference_identity_hits_can_answer_without_fts(
            request,
            identity,
            identity_hits.len(),
            saturated,
        ) {
            dedupe_sort_truncate(&mut identity_hits, request.limit);
            return Ok(identity_hits);
        }
    }

    let mut hits = reference_rows_to_hits(
        status,
        request,
        search_reference_fts_rows(connection, status, request)?,
    );
    hits.extend(identity_hits);

    Ok(hits)
}

fn search_reference_identity_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
) -> Result<ReferenceIdentityRows, StorageError> {
    let path_filter = path_filter_sql_for_column("r.path", status, request);
    let language_filter = language_filter_sql_for_column("f.language_id", status, request);
    let direct_limit = reference_identity_candidate_limit(request);
    let sql = reference_rows_sql(&format!(
        "
          AND r.name = ?
          {path_filter}
          {language_filter}
        "
    ));
    let mut values = vec![
        Value::Text(required_scope(status)?.to_owned()),
        Value::Text(identity.leaf_name().to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer((direct_limit + 1) as i64));

    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(params_from_iter(values), row_to_reference)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?;
    let saturated = rows.len() > direct_limit;
    rows.truncate(direct_limit);

    Ok(ReferenceIdentityRows { rows, saturated })
}

fn search_reference_fts_rows(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<ReferenceRow>, StorageError> {
    let fts_query = fts_match_query(&request.query);
    let fts_filter = fts_path_and_language_filter_sql(status, request);
    let sql = reference_rows_sql(&format!(
        "
          AND r.reference_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'reference'
                {fts_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
              LIMIT ?
          )
        "
    ));
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            &fts_query,
            candidate_limit(request, CandidateLayer::Reference),
            candidate_limit(request, CandidateLayer::Reference),
        )),
        row_to_reference,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn reference_rows_sql(predicate_sql: &str) -> String {
    format!(
        "
        SELECT r.file_id, r.path, f.language_id, r.name, r.kind,
               r.target_symbol_snapshot_id, r.byte_start, r.byte_end,
               r.line_start, r.line_end, r.target_hint, r.resolution_state,
               r.confidence_basis_points, r.confidence_tier, s.canonical_symbol_id
        FROM code_repository_references r
        INNER JOIN code_repository_files f
            ON f.source_scope = r.source_scope AND f.path = r.path
        LEFT JOIN code_repository_symbols s
            ON s.source_scope = r.source_scope
           AND s.symbol_snapshot_id = r.target_symbol_snapshot_id
        WHERE r.source_scope = ?
          {predicate_sql}
        ORDER BY r.path ASC, r.line_start ASC
        LIMIT ?
        "
    )
}

fn row_to_reference(row: &Row<'_>) -> rusqlite::Result<ReferenceRow> {
    Ok(ReferenceRow {
        file_id: row.get(0)?,
        path: row.get(1)?,
        language_id: row.get(2)?,
        name: row.get(3)?,
        kind: row.get(4)?,
        target_symbol_snapshot_id: row.get(5)?,
        byte_range: RepositoryCodeRange {
            start: row.get(6)?,
            end: row.get(7)?,
        },
        line_range: RepositoryCodeRange {
            start: row.get(8)?,
            end: row.get(9)?,
        },
        target_hint: row.get(10)?,
        resolution_state: row.get(11)?,
        confidence_basis_points: row.get(12)?,
        confidence_tier: row.get(13)?,
        target_canonical_symbol_id: row.get(14)?,
    })
}

fn reference_rows_to_hits(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    rows: Vec<ReferenceRow>,
) -> Vec<CodeRetrievalHit> {
    let score_query = ScoreQuery::new(&request.query);

    rows.into_iter()
        .filter(|row| selected_row(&row.path, &row.language_id, status, request))
        .filter_map(|row| {
            let score = score_query.score([
                row.name.as_str(),
                row.kind.as_str(),
                row.target_hint.as_deref().unwrap_or_default(),
                row.target_canonical_symbol_id
                    .as_deref()
                    .unwrap_or_default(),
            ]) + scoped_identity_query_bonus(
                &request.query,
                [
                    row.target_hint.as_deref().unwrap_or_default(),
                    row.target_canonical_symbol_id
                        .as_deref()
                        .unwrap_or_default(),
                ],
            );
            (score > 0.0).then(|| {
                hit_from_parts(
                    status,
                    HitParts {
                        path: row.path,
                        language_id: row.language_id,
                        byte_range: row.byte_range,
                        line_range: row.line_range,
                        symbol_snapshot_id: row.target_symbol_snapshot_id,
                        canonical_symbol_id: row.target_canonical_symbol_id,
                        file_id: Some(row.file_id),
                        retrieval_layers: vec![CodeRetrievalLayer::Reference],
                        score: score + 1.5,
                        excerpt: format!("{} reference to {}", row.kind, row.name),
                        degraded_reason: None,
                        edge_kind: Some(row.kind),
                        edge_resolution_state: Some(row.resolution_state),
                        edge_target_hint: row.target_hint,
                        edge_confidence_basis_points: Some(row.confidence_basis_points),
                        edge_confidence_tier: Some(row.confidence_tier),
                    },
                )
            })
        })
        .collect()
}

fn reference_identity_hits_can_answer_without_fts(
    request: &CodeRetrievalRequest,
    identity: &SymbolIdentityQuery,
    hit_count: usize,
    saturated: bool,
) -> bool {
    hit_count > 0
        && !saturated
        && request.code_query_kind == CodeQueryKind::References
        && (identity.is_scoped()
            || (hit_count <= request.limit
                && specific_reference_identity_leaf(identity.leaf_name())))
}

fn reference_identity_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    candidate_limit(request, CandidateLayer::Reference).min(200)
}

fn specific_reference_identity_leaf(leaf_name: &str) -> bool {
    leaf_name.len() >= 8 || leaf_name.contains('_') || has_case_boundary(leaf_name)
}

fn has_case_boundary(value: &str) -> bool {
    let mut previous: Option<char> = None;
    for character in value.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            return true;
        }
        previous = Some(character);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

    #[test]
    fn reference_identity_fast_path_requires_specific_bounded_hits() {
        let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
        let request = CodeRetrievalRequest::new(
            "TargetThing",
            selector,
            CodeQueryKind::References,
            10,
            FreshnessPolicy::AllowStale,
        )
        .expect("request should validate");
        let identity =
            SymbolIdentityQuery::from_query("TargetThing").expect("identity query should parse");

        assert!(reference_identity_hits_can_answer_without_fts(
            &request, &identity, 3, false
        ));
        assert!(!reference_identity_hits_can_answer_without_fts(
            &request, &identity, 11, false
        ));
        assert!(!reference_identity_hits_can_answer_without_fts(
            &request, &identity, 3, true
        ));
        let broad_identity =
            SymbolIdentityQuery::from_query("State").expect("identity query should parse");
        assert!(!reference_identity_hits_can_answer_without_fts(
            &request,
            &broad_identity,
            1,
            false
        ));
    }
}
