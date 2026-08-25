use rusqlite::{Connection, params_from_iter, types::Value};

use crate::storage::sqlite::code::search::EXACT_SEARCH_OWNER_PREDICATE_SQL;
use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest,
        RepositoryCodeRange,
    },
    storage::StorageError,
};

use super::super::{
    hits::{
        HitParts, dedupe_sort_truncate, filtered_hits_for_gate, hit_from_parts, required_scope,
        selected_row,
    },
    prepare::prepare_code_search_statement,
};
use super::super::{
    hybrid::chunk_gate::{
        hybrid_hits_can_answer_without_graph_expansion, retain_query_language_scoped_workflow_hits,
    },
    hybrid::exact_path::hybrid_query_should_use_layered_chunk_search,
    hybrid::planning::{
        hybrid_query_prefers_chunk_first, query_language_scoped_workflow_surface_scopes,
        strict_hybrid_chunk_candidate_limit, workflow_language_scope_language_ids,
    },
    relevance::{
        CandidateLayer, ScoreQuery, candidate_limit, chunk_layers_for_request,
        compound_hybrid_chunk_fts_match_query, declaration_chunk_bonus,
        direct_hybrid_chunk_fts_match_query, focused_hybrid_chunk_fts_match_query,
        fts_path_and_language_filter_sql, hybrid_chunk_fts_match_query,
        lifecycle_hybrid_chunk_fts_match_query, push_language_filter_values,
        push_path_filter_values, push_query_path_substring_filter_values, query_terms,
        score_exact_path, strict_hybrid_chunk_fts_match_query,
        structured_hybrid_chunk_fts_match_query, symbol_query_bonus,
    },
    rows::ChunkRow,
    scoring::api_sequence::compact_unique_api_sequence_chunk_bonus,
    scoring::chunk_path::hybrid_chunk_path_adjustment,
    scoring::designated_initializer::designated_initializer_chunk_bonus,
    scoring::flow::{
        compact_api_sequence_chunk_bonus, compact_high_coverage_chunk_bonus,
        execution_flow_chunk_bonus, inline_construct_chunk_bonus,
        source_definition_body_chunk_bonus,
    },
    scoring::inline_usage::language_scoped_inline_usage_chunk_bonus,
    scoring::interface::public_interface_chunk_bonus,
    scoring::path_ranking::declaration_surface_path_bonus,
    scoring::proximity::query_proximity_chunk_bonus,
};
use super::{
    exact_definition_chunk_bonus, exact_reference_chunk_bonus,
    exact_reference_chunk_contains_usage, reference_usage_language_filter_sql,
};

pub(in crate::storage::sqlite::code::query) fn search_chunks(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let chunk_first = request.code_query_kind == CodeQueryKind::Hybrid
        && hybrid_query_prefers_chunk_first(request)
        && hybrid_query_should_use_layered_chunk_search(request);
    let narrow_chunk_candidate_limit = hybrid_narrow_chunk_candidate_limit(request);
    let broad_chunk_candidate_limit = candidate_limit(request, CandidateLayer::Chunk);
    let mut narrow_hits = Vec::new();
    if request.code_query_kind == CodeQueryKind::Hybrid {
        if let Some(strict_fts_query) = strict_hybrid_chunk_fts_match_query(&request.query) {
            let mut hits = search_chunks_with_fts_query(
                connection,
                status,
                request,
                &strict_fts_query,
                strict_hybrid_chunk_candidate_limit(request),
            )?;
            retain_query_language_scoped_workflow_hits(request, &mut hits);
            let filtered_hits = filtered_hits_for_gate(&hits, request);
            if hybrid_hits_can_answer_without_graph_expansion(request, &filtered_hits) {
                return Ok(hits);
            }
            narrow_hits =
                merge_strict_and_broad_chunk_hits(narrow_hits, hits, narrow_chunk_candidate_limit);
        }
    }

    if chunk_first
        && let Some(structured_fts_query) = structured_hybrid_chunk_fts_match_query(&request.query)
    {
        let mut hits = search_chunks_with_fts_query(
            connection,
            status,
            request,
            &structured_fts_query,
            narrow_chunk_candidate_limit,
        )?;
        retain_query_language_scoped_workflow_hits(request, &mut hits);
        narrow_hits =
            merge_strict_and_broad_chunk_hits(narrow_hits, hits, narrow_chunk_candidate_limit);
        let filtered_narrow_hits = filtered_hits_for_gate(&narrow_hits, request);
        if hybrid_hits_can_answer_without_graph_expansion(request, &filtered_narrow_hits) {
            return Ok(narrow_hits);
        }
    }

    if chunk_first
        && let Some(focused_fts_query) = focused_hybrid_chunk_fts_match_query(&request.query)
    {
        let mut hits = search_chunks_with_fts_query(
            connection,
            status,
            request,
            &focused_fts_query,
            narrow_chunk_candidate_limit,
        )?;
        retain_query_language_scoped_workflow_hits(request, &mut hits);
        narrow_hits =
            merge_strict_and_broad_chunk_hits(narrow_hits, hits, narrow_chunk_candidate_limit);
    }

    if chunk_first
        && let Some(lifecycle_fts_query) = lifecycle_hybrid_chunk_fts_match_query(&request.query)
    {
        let mut hits = search_chunks_with_fts_query(
            connection,
            status,
            request,
            &lifecycle_fts_query,
            narrow_chunk_candidate_limit,
        )?;
        retain_query_language_scoped_workflow_hits(request, &mut hits);
        narrow_hits =
            merge_strict_and_broad_chunk_hits(narrow_hits, hits, narrow_chunk_candidate_limit);
    }

    if chunk_first
        && let Some(compound_fts_query) = compound_hybrid_chunk_fts_match_query(&request.query)
    {
        let mut hits = search_chunks_with_fts_query(
            connection,
            status,
            request,
            &compound_fts_query,
            narrow_chunk_candidate_limit,
        )?;
        retain_query_language_scoped_workflow_hits(request, &mut hits);
        narrow_hits =
            merge_strict_and_broad_chunk_hits(narrow_hits, hits, narrow_chunk_candidate_limit);
    }
    if chunk_first && !narrow_hits.is_empty() {
        let filtered_narrow_hits = filtered_hits_for_gate(&narrow_hits, request);
        if hybrid_hits_can_answer_without_graph_expansion(request, &filtered_narrow_hits) {
            return Ok(narrow_hits);
        }
    }

    let fts_query = if request.code_query_kind == CodeQueryKind::Hybrid {
        direct_hybrid_chunk_fts_match_query(&request.query)
    } else {
        hybrid_chunk_fts_match_query(&request.query)
    };
    let mut hits = search_chunks_with_fts_query(
        connection,
        status,
        request,
        &fts_query,
        broad_chunk_candidate_limit,
    )?;
    if !narrow_hits.is_empty() {
        hits = merge_strict_and_broad_chunk_hits(narrow_hits, hits, broad_chunk_candidate_limit);
    }

    Ok(hits)
}

fn hybrid_narrow_chunk_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    if request.code_query_kind == CodeQueryKind::Hybrid
        && hybrid_query_prefers_chunk_first(request)
        && hybrid_query_should_use_layered_chunk_search(request)
    {
        strict_hybrid_chunk_candidate_limit(request)
    } else {
        candidate_limit(request, CandidateLayer::Chunk)
    }
}

fn merge_strict_and_broad_chunk_hits(
    strict_hits: Vec<CodeRetrievalHit>,
    mut broad_hits: Vec<CodeRetrievalHit>,
    candidate_limit: usize,
) -> Vec<CodeRetrievalHit> {
    if strict_hits.is_empty() {
        return broad_hits;
    }
    broad_hits.extend(strict_hits);
    dedupe_sort_truncate(&mut broad_hits, candidate_limit);
    broad_hits
}

fn search_chunks_with_fts_query(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    fts_query: &str,
    fts_limit: usize,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let query_language_filters = query_language_scoped_workflow_language_filters(request);
    let fts_filter =
        chunk_fts_path_and_language_filter_sql(status, request, &query_language_filters);
    let exclude_generated_flag = usize::from(request.exclude_generated);
    let sql = format!(
        "
        SELECT c.file_id, c.path, c.language_id, c.content, c.byte_start, c.byte_end,
               c.line_start, c.line_end, c.symbol_snapshot_id,
               symbol.canonical_symbol_id, symbol.name, symbol.qualified_name,
               f.parse_status, f.degraded_reason, f.is_generated
        FROM code_repository_chunks c
        INNER JOIN code_repository_files f
            ON f.source_scope = c.source_scope AND f.path = c.path
        LEFT JOIN code_repository_symbols symbol
            ON symbol.source_scope = c.source_scope
           AND symbol.symbol_snapshot_id = c.symbol_snapshot_id
        WHERE c.source_scope = ?
          AND c.chunk_id IN (
              SELECT record_id
              FROM code_repository_search
              WHERE code_repository_search MATCH ?
                AND source_scope = ?
                AND document_kind = 'chunk'
                {EXACT_SEARCH_OWNER_PREDICATE_SQL}
                {fts_filter}
                AND ({exclude_generated_flag} = 0 OR NOT EXISTS (SELECT 1 FROM code_repository_files fts_file WHERE fts_file.source_scope = code_repository_search.source_scope AND fts_file.path = code_repository_search.path AND fts_file.is_generated != 0))
              ORDER BY coalesce((SELECT fts_file.is_generated FROM code_repository_files fts_file WHERE fts_file.source_scope = code_repository_search.source_scope AND fts_file.path = code_repository_search.path LIMIT 1), 0) ASC,
                  bm25(code_repository_search) ASC,
                  record_id ASC
              LIMIT ?
          )
        ORDER BY c.path ASC, c.line_start ASC
        LIMIT ?
        "
    );
    let mut statement = prepare_code_search_statement(connection, &sql)?;
    let rows = statement.query_map(
        params_from_iter(chunk_fts_values_for_limited_with_language(
            required_scope(status)?,
            status,
            request,
            fts_query,
            &query_language_filters,
            fts_limit,
            fts_limit,
        )),
        |row| {
            Ok(ChunkRow {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language_id: row.get(2)?,
                content: row.get(3)?,
                byte_range: RepositoryCodeRange {
                    start: row.get(4)?,
                    end: row.get(5)?,
                },
                line_range: RepositoryCodeRange {
                    start: row.get(6)?,
                    end: row.get(7)?,
                },
                symbol_snapshot_id: row.get(8)?,
                canonical_symbol_id: row.get(9)?,
                symbol_name: row.get(10)?,
                symbol_qualified_name: row.get(11)?,
                parse_status: row.get(12)?,
                degraded_reason: row.get(13)?,
                is_generated: row.get::<_, i64>(14)? != 0,
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let score_query = ScoreQuery::new(&request.query);
    let declaration_terms = query_terms(&query);
    let mut hits = Vec::new();
    for row in rows {
        let row = row.map_err(StorageError::from)?;
        if !selected_row(
            &row.path,
            &row.language_id,
            row.is_generated,
            status,
            request,
        ) {
            continue;
        }
        if !exact_reference_chunk_contains_usage(request, &row.language_id, &row.content) {
            continue;
        }
        let declaration_bonus = declaration_chunk_bonus(&declaration_terms, &row.content);
        let symbol_bonus = row.symbol_name.as_deref().map_or(0.0, |name| {
            symbol_query_bonus(
                &request.query,
                name,
                row.symbol_qualified_name.as_deref().unwrap_or_default(),
                "",
                row.canonical_symbol_id.as_deref().unwrap_or_default(),
                request,
            )
        });
        let score = score_query.score([row.content.as_str(), row.path.as_str()])
            + score_exact_path(&query, &row.path)
            + declaration_bonus
            + exact_definition_chunk_bonus(request, &row.content)
            + declaration_surface_path_bonus(declaration_bonus, &row.path, request)
            + symbol_bonus;
        let score = score
            + exact_reference_chunk_bonus(request, score, &row.content)
            + compact_high_coverage_chunk_bonus(
                score,
                &request.query,
                &row.content,
                &row.path,
                request,
            )
            + compact_api_sequence_chunk_bonus(
                score,
                &request.query,
                &row.content,
                &row.path,
                request,
            )
            + compact_unique_api_sequence_chunk_bonus(
                score,
                &request.query,
                &row.content,
                &row.path,
                request,
            )
            + query_proximity_chunk_bonus(score, &request.query, &row.content, &row.path, request)
            + public_interface_chunk_bonus(score, &request.query, &row.content, &row.path, request)
            + execution_flow_chunk_bonus(score, &request.query, &row.content, &row.path, request)
            + designated_initializer_chunk_bonus(
                score,
                &request.query,
                &row.content,
                &row.path,
                request,
            )
            + hybrid_chunk_path_adjustment(score, &request.query, &row.content, &row.path, request)
            + inline_construct_chunk_bonus(score, &request.query, &row.content, &row.path, request)
            + language_scoped_inline_usage_chunk_bonus(
                score,
                &request.query,
                &row.content,
                &row.path,
                &row.language_id,
                request,
            )
            + source_definition_body_chunk_bonus(
                score,
                &request.query,
                &row.content,
                &row.path,
                request,
            );
        if score <= 0.0 {
            continue;
        }
        hits.push(hit_from_parts(
            status,
            HitParts {
                path: row.path,
                language_id: row.language_id,
                byte_range: row.byte_range,
                line_range: row.line_range,
                symbol_snapshot_id: row.symbol_snapshot_id,
                canonical_symbol_id: row.canonical_symbol_id,
                file_id: Some(row.file_id),
                retrieval_layers: chunk_layers_for_request(request, &row.parse_status),
                score,
                excerpt: row.content,
                is_generated: row.is_generated,
                degraded_reason: row.degraded_reason,
                edge_kind: None,
                edge_resolution_state: None,
                edge_target_hint: None,
                edge_confidence_basis_points: None,
                edge_confidence_tier: None,
            },
        ));
    }

    Ok(hits)
}

fn query_language_scoped_workflow_language_filters(request: &CodeRetrievalRequest) -> Vec<String> {
    let mut language_filters = Vec::new();
    for scope in query_language_scoped_workflow_surface_scopes(request) {
        for language_id in workflow_language_scope_language_ids(scope) {
            if !language_filters.iter().any(|filter| filter == language_id) {
                language_filters.push((*language_id).to_owned());
            }
        }
    }

    language_filters
}

fn chunk_fts_path_and_language_filter_sql(
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    query_language_filters: &[String],
) -> String {
    let mut filter = fts_path_and_language_filter_sql(status, request);
    append_chunk_filter(&mut filter, reference_usage_language_filter_sql(request));
    let extra_filter = exact_language_filter_sql("language_id", query_language_filters.len());
    append_chunk_filter(&mut filter, extra_filter);

    filter
}

fn append_chunk_filter(filter: &mut String, clause: String) {
    if clause.is_empty() {
        return;
    }
    if filter.is_empty() {
        *filter = format!("AND {clause}");
    } else {
        filter.push_str(" AND ");
        filter.push_str(&clause);
    }
}

fn exact_language_filter_sql(column: &str, filter_count: usize) -> String {
    if filter_count == 0 {
        return String::new();
    }

    let clauses = std::iter::repeat_with(|| format!("{column} = ?"))
        .take(filter_count)
        .collect::<Vec<_>>();
    format!("({})", clauses.join(" OR "))
}

fn chunk_fts_values_for_limited_with_language(
    source_scope: &str,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    fts_query: &str,
    query_language_filters: &[String],
    fts_limit: usize,
    limit: usize,
) -> Vec<Value> {
    let mut values = vec![
        Value::Text(source_scope.to_owned()),
        Value::Text(fts_query.to_owned()),
        Value::Text(source_scope.to_owned()),
    ];
    push_path_filter_values(&mut values, &status.path_filters);
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_query_path_substring_filter_values(&mut values, &request.query_path_substrings);
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, &request.query_language_filters);
    push_language_filter_values(&mut values, query_language_filters);
    values.push(Value::Integer(fts_limit as i64));
    values.push(Value::Integer(limit as i64));

    values
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
