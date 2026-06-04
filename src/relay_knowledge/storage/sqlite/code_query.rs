use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

#[path = "code_query_api_identities.rs"]
mod code_query_api_identities;
#[path = "code_query_api_sequence_scoring.rs"]
mod code_query_api_sequence_scoring;
#[path = "code_query_call_counts.rs"]
mod code_query_call_counts;
#[path = "code_query_call_direction.rs"]
mod code_query_call_direction;
#[path = "code_query_call_target_ranking.rs"]
mod code_query_call_target_ranking;
#[path = "code_query_calls.rs"]
mod code_query_calls;
#[path = "code_query_designated_initializer_scoring.rs"]
mod code_query_designated_initializer_scoring;
#[path = "code_query_excerpts.rs"]
mod code_query_excerpts;
#[path = "code_query_flow_scoring.rs"]
mod code_query_flow_scoring;
#[path = "code_query_hybrid_direct_gate.rs"]
mod code_query_hybrid_direct_gate;
#[path = "code_query_hybrid_exact_path.rs"]
mod code_query_hybrid_exact_path;
#[path = "code_query_hybrid_planning.rs"]
mod code_query_hybrid_planning;
#[path = "code_query_identifiers.rs"]
mod code_query_identifiers;
#[path = "code_query_import_scoring.rs"]
mod code_query_import_scoring;
#[path = "code_query_import_targets.rs"]
mod code_query_import_targets;
#[path = "code_query_imports.rs"]
mod code_query_imports;
#[path = "code_query_line_ranges.rs"]
mod code_query_line_ranges;
#[path = "code_query_path_ranking.rs"]
mod code_query_path_ranking;
#[path = "code_query_proximity_scoring.rs"]
mod code_query_proximity_scoring;
#[path = "code_query_references.rs"]
mod code_query_references;
#[path = "code_query_rows.rs"]
mod code_query_rows;
#[path = "code_query_sbom.rs"]
mod code_query_sbom;
#[path = "code_query_support.rs"]
mod code_query_support;
#[path = "code_query_symbols.rs"]
mod code_query_symbols;

use crate::{
    domain::{
        CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer,
        CodeRetrievalRequest, RepositoryCodeRange,
    },
    storage::StorageError,
};

#[cfg(test)]
const MAX_CANDIDATE_BIND_VALUES: usize = 900;

use super::code_query_hits::selected_row;
pub(super) use super::code_query_hits::{
    HitParts, chunk_layers, dedupe_sort_truncate, hit_from_parts, mark_hits_degraded,
    required_repository, required_scope,
};
use super::code_query_prepare::{
    code_search_error_can_use_empty_results, code_search_plannable_outage_reason,
    code_search_read_model_unavailable_reason, prepare_code_search_statement,
    retry_code_search_operation,
};
#[cfg(test)]
use super::code_query_scope::path_matches_filter;
pub(super) use super::code_query_scope::{language_filter_allows, path_filter_allows};
use code_query_api_sequence_scoring::compact_unique_api_sequence_chunk_bonus;
use code_query_calls::search_calls;
use code_query_designated_initializer_scoring::designated_initializer_chunk_bonus;
use code_query_flow_scoring::{
    compact_api_sequence_chunk_bonus, compact_high_coverage_chunk_bonus,
    execution_flow_chunk_bonus, inline_construct_chunk_bonus, source_definition_body_chunk_bonus,
};
use code_query_hybrid_direct_gate::hybrid_direct_results_can_answer_without_graph_expansion;
use code_query_hybrid_exact_path::{
    hybrid_exact_path_query_can_defer_to_source_fallback, hybrid_query_can_skip_graph_expansion,
    hybrid_query_should_use_layered_chunk_search,
};
use code_query_hybrid_planning::{
    hybrid_query_prefers_chunk_first, hybrid_sequence_terms,
    query_language_scoped_workflow_surface_scopes, workflow_language_scope_language_ids,
    workflow_language_scope_matches,
};
#[cfg(test)]
use code_query_import_scoring::{
    import_public_dependency_surface_bonus, import_reexport_surface_penalty,
    import_self_implementation_penalty, import_source_path_query_overlap_bonus,
    import_surface_bonus, import_target_symbol_bonus,
};
#[cfg(test)]
use code_query_import_targets::target_symbol_import_query;
use code_query_imports::search_imports;
use code_query_path_ranking::declaration_surface_path_bonus;
use code_query_proximity_scoring::query_proximity_chunk_bonus;
use code_query_references::{reference_usage_context_bonus, search_references};
use code_query_rows::ChunkRow;
use code_query_sbom::search_sbom;
use code_query_support::*;
use code_query_symbols::{
    hybrid_symbol_query_can_answer_without_non_symbol_layers, search_symbols,
};

const STRICT_HYBRID_CHUNK_LIMIT_MULTIPLIER: usize = 6;
const STRICT_HYBRID_CHUNK_MIN_CANDIDATES: usize = 40;
const STRICT_HYBRID_CHUNK_MAX_CANDIDATES: usize = 120;

pub(super) fn search_code(
    connection: &mut Connection,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    match retry_code_search_operation(|| search_code_with_status(connection, &status, &request)) {
        Ok(hits) => Ok(hits),
        Err(error) if code_search_error_can_use_empty_results(&request, &error) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

pub(super) fn search_code_scope(
    connection: &mut Connection,
    source_scope: &str,
    request: CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status =
        super::code_status::repository_scope_status_by_source_scope(connection, source_scope)?
            .ok_or_else(|| {
                StorageError::InvalidInput(format!(
                    "code repository source scope '{source_scope}' is not indexed"
                ))
            })?;

    match retry_code_search_operation(|| search_code_with_status(connection, &status, &request)) {
        Ok(hits) => Ok(hits),
        Err(error) if code_search_error_can_use_empty_results(&request, &error) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn search_code_with_status(
    connection: &mut Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    if request.code_query_kind == CodeQueryKind::Impact {
        return Err(StorageError::InvalidInput(
            "impact query kind requires repo impact with base/head refs".to_owned(),
        ));
    }
    if request.code_query_kind == CodeQueryKind::Sbom {
        return search_sbom(connection, status, request);
    }
    let mut hits = Vec::new();
    let mut searched_chunks = false;
    let mut chunk_first_outage = None;
    if request.code_query_kind == CodeQueryKind::Hybrid
        && hybrid_query_prefers_chunk_first(request)
        && hybrid_query_should_use_layered_chunk_search(request)
    {
        match search_chunks(connection, status, request) {
            Ok(mut chunk_hits) => {
                searched_chunks = true;
                retain_query_language_scoped_workflow_hits(request, &mut chunk_hits);
                if hybrid_chunk_results_can_answer_without_graph_expansion(request, &chunk_hits)
                    || hybrid_direct_results_can_answer_without_graph_expansion(
                        request,
                        &chunk_hits,
                    )
                {
                    hits.extend(chunk_hits);
                    dedupe_sort_truncate(&mut hits, request.limit);
                    return Ok(hits);
                }
                hits.extend(chunk_hits);
            }
            Err(error) => {
                let Some(reason) = hybrid_chunk_first_search_outage_reason(request, &error) else {
                    return Err(error);
                };
                searched_chunks = true;
                chunk_first_outage = Some((reason, error));
            }
        }
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Hybrid | CodeQueryKind::Symbol | CodeQueryKind::Definition
    ) {
        hits.extend(search_symbols(
            connection,
            status,
            request,
            chunk_first_outage.as_ref().map(|outage| outage.0.as_str()),
        )?);
        if hybrid_symbol_query_can_answer_without_non_symbol_layers(request, &hits) {
            dedupe_sort_truncate(&mut hits, request.limit);
            return Ok(hits);
        }
    }
    if definition_query_needs_chunk_fallback(request, &hits) {
        let chunk_hits = search_chunks(connection, status, request);
        if let Some(partial_hits) =
            append_hits_or_return_partial_on_search_outage(&mut hits, request, chunk_hits)?
        {
            return Ok(partial_hits);
        }
    }
    if request.code_query_kind == CodeQueryKind::Hybrid {
        if hybrid_exact_path_query_can_defer_to_source_fallback(request, &hits) {
            dedupe_sort_truncate(&mut hits, request.limit);
            return Ok(hits);
        }
        if !searched_chunks {
            let chunk_hits = search_chunks(connection, status, request);
            if let Some(partial_hits) =
                append_hits_or_return_partial_on_search_outage(&mut hits, request, chunk_hits)?
            {
                return Ok(partial_hits);
            }
        }
        if hybrid_chunk_results_can_answer_without_graph_expansion(request, &hits)
            || hybrid_direct_results_can_answer_without_graph_expansion(request, &hits)
        {
            dedupe_sort_truncate(&mut hits, request.limit);
            return Ok(hits);
        }
        if hybrid_query_can_skip_graph_expansion(request, &hits) {
            dedupe_sort_truncate(&mut hits, request.limit);
            return Ok(hits);
        }
        let reference_hits = search_references(connection, status, request);
        if let Some(partial_hits) =
            append_hits_or_return_partial_on_search_outage(&mut hits, request, reference_hits)?
        {
            return Ok(partial_hits);
        }
        let call_hits = search_calls(connection, status, request);
        if let Some(partial_hits) =
            append_hits_or_return_partial_on_search_outage(&mut hits, request, call_hits)?
        {
            return Ok(partial_hits);
        }
        let import_hits = search_imports(connection, status, request);
        if let Some(partial_hits) =
            append_hits_or_return_partial_on_search_outage(&mut hits, request, import_hits)?
        {
            return Ok(partial_hits);
        }
        if let Some((reason, error)) = chunk_first_outage {
            if hits.is_empty() {
                return Err(error);
            }
            mark_hits_degraded(&mut hits, &reason);
        }
        dedupe_sort_truncate(&mut hits, request.limit);
        return Ok(hits);
    }
    if request.code_query_kind == CodeQueryKind::References {
        hits.extend(search_references(connection, status, request)?);
    }
    if references_query_needs_chunk_fallback(request, &hits) {
        hits.extend(search_chunks(connection, status, request)?);
    }
    if matches!(
        request.code_query_kind,
        CodeQueryKind::Callers | CodeQueryKind::Callees
    ) {
        hits.extend(search_calls(connection, status, request)?);
    }
    if request.code_query_kind == CodeQueryKind::Imports {
        hits.extend(search_imports(connection, status, request)?);
    }
    dedupe_sort_truncate(&mut hits, request.limit);

    Ok(hits)
}

fn append_hits_or_return_partial_on_search_outage(
    hits: &mut Vec<CodeRetrievalHit>,
    request: &CodeRetrievalRequest,
    layer_hits: Result<Vec<CodeRetrievalHit>, StorageError>,
) -> Result<Option<Vec<CodeRetrievalHit>>, StorageError> {
    match layer_hits {
        Ok(layer_hits) => {
            hits.extend(layer_hits);
            Ok(None)
        }
        Err(error) if !hits.is_empty() => {
            let Some(reason) = code_search_plannable_outage_reason(request, &error)
                .or_else(|| hybrid_chunk_first_search_outage_reason(request, &error))
            else {
                return Err(error);
            };
            mark_hits_degraded(hits, &reason);
            dedupe_sort_truncate(hits, request.limit);
            Ok(Some(std::mem::take(hits)))
        }
        Err(error) => Err(error),
    }
}

fn hybrid_chunk_first_search_outage_reason(
    request: &CodeRetrievalRequest,
    error: &StorageError,
) -> Option<String> {
    (request.code_query_kind == CodeQueryKind::Hybrid && hybrid_query_prefers_chunk_first(request))
        .then(|| code_search_read_model_unavailable_reason(error))
        .flatten()
}

fn definition_query_needs_chunk_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Definition {
        return false;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return hits.is_empty();
    };

    !hits.iter().any(|hit| {
        hit.canonical_symbol_id
            .as_deref()
            .is_some_and(|symbol_id| canonical_symbol_leaf_matches(symbol_id, identity.leaf_name()))
    })
}

fn references_query_needs_chunk_fallback(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    request.code_query_kind == CodeQueryKind::References
        && hits.is_empty()
        && SymbolIdentityQuery::from_query(&request.query).is_some()
}

fn canonical_symbol_leaf_matches(canonical_symbol_id: &str, leaf_name: &str) -> bool {
    canonical_symbol_id
        .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .find(|part| !part.is_empty())
        .is_some_and(|part| part == leaf_name)
}

fn exact_reference_chunk_bonus(
    request: &CodeRetrievalRequest,
    base_score: f64,
    content: &str,
) -> f64 {
    if request.code_query_kind != CodeQueryKind::References {
        return 0.0;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return 0.0;
    };

    reference_usage_context_bonus(
        base_score,
        "value",
        identity.leaf_name(),
        Some(content),
        request,
    )
}

fn exact_definition_chunk_bonus(request: &CodeRetrievalRequest, content: &str) -> f64 {
    if request.code_query_kind != CodeQueryKind::Definition {
        return 0.0;
    }
    let Some(identity) = SymbolIdentityQuery::from_query(&request.query) else {
        return 0.0;
    };

    if content
        .lines()
        .map(str::trim)
        .any(|line| declaration_line_defines_identity(line, identity.leaf_name()))
    {
        3.0
    } else {
        0.0
    }
}

fn declaration_line_defines_identity(line: &str, leaf_name: &str) -> bool {
    if !line_contains_identifier(line, leaf_name) {
        return false;
    }
    if line.starts_with("typedef ") || line.contains(" typedef ") {
        return true;
    }
    if line
        .strip_prefix("using ")
        .is_some_and(|remainder| line_starts_with_identifier(remainder, leaf_name))
    {
        return true;
    }

    ["struct ", "class ", "enum ", "union "]
        .into_iter()
        .filter_map(|prefix| line.strip_prefix(prefix))
        .any(|remainder| line_starts_with_identifier(remainder, leaf_name))
}

fn hybrid_chunk_results_can_answer_without_graph_expansion(
    request: &CodeRetrievalRequest,
    hits: &[CodeRetrievalHit],
) -> bool {
    if request.code_query_kind != CodeQueryKind::Hybrid {
        return false;
    }
    let terms = hybrid_sequence_terms(&request.query);
    if terms.len() < 3 {
        return false;
    }
    let language_scopes = query_language_scoped_workflow_surface_scopes(request);
    let required_matches = terms.len().clamp(3, 4);
    let required_hits = request.limit.clamp(1, 3);
    let dense_chunk_hits = hits
        .iter()
        .filter(|hit| {
            hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
                && !hit
                    .retrieval_layers
                    .contains(&CodeRetrievalLayer::TextFallback)
                && workflow_language_scopes_allow_hit(&language_scopes, &hit.language_id)
                && hybrid_sequence_match_count(&hit.excerpt, &terms) >= required_matches
        })
        .take(required_hits)
        .count();
    if dense_chunk_hits >= required_hits {
        return true;
    }

    hybrid_chunk_results_have_collective_dense_coverage(
        &terms,
        hits,
        required_hits,
        &language_scopes,
    )
}

fn hybrid_chunk_results_have_collective_dense_coverage(
    terms: &[String],
    hits: &[CodeRetrievalHit],
    required_hits: usize,
    language_scopes: &[&str],
) -> bool {
    let required_coverage = terms.len().saturating_mul(2).div_ceil(3).max(4);
    let required_dense_matches = terms.len().clamp(3, 4);
    let mut covered_terms = Vec::new();
    let mut supporting_hits = 0usize;
    let mut has_dense_hit = false;
    for hit in hits {
        if !hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
            || hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::TextFallback)
            || !workflow_language_scopes_allow_hit(language_scopes, &hit.language_id)
        {
            continue;
        }
        let excerpt = hit.excerpt.to_ascii_lowercase();
        let mut matched_terms = 0usize;
        for term in terms {
            if excerpt.contains(term.as_str()) {
                matched_terms += 1;
                if !covered_terms.contains(term) {
                    covered_terms.push(term.clone());
                }
            }
        }
        if matched_terms >= 2 {
            supporting_hits += 1;
        }
        has_dense_hit |= matched_terms >= required_dense_matches;
    }

    supporting_hits >= required_hits && has_dense_hit && covered_terms.len() >= required_coverage
}

fn workflow_language_scopes_allow_hit(language_scopes: &[&str], language_id: &str) -> bool {
    language_scopes.is_empty()
        || language_scopes
            .iter()
            .any(|scope| workflow_language_scope_matches(language_id, scope))
}

fn retain_query_language_scoped_workflow_hits(
    request: &CodeRetrievalRequest,
    hits: &mut Vec<CodeRetrievalHit>,
) {
    let language_scopes = query_language_scoped_workflow_surface_scopes(request);
    if language_scopes.is_empty() {
        return;
    }

    hits.retain(|hit| workflow_language_scopes_allow_hit(&language_scopes, &hit.language_id));
}

fn hybrid_sequence_match_count(excerpt: &str, terms: &[String]) -> usize {
    let excerpt = excerpt.to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| excerpt.contains(term.as_str()))
        .count()
}

fn line_starts_with_identifier(line: &str, identifier: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(identifier)
        && trimmed
            .get(identifier.len()..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
}

fn line_contains_identifier(line: &str, identifier: &str) -> bool {
    line.match_indices(identifier).any(|(start, _)| {
        let end = start + identifier.len();
        line.get(..start).is_some_and(|prefix| {
            prefix
                .chars()
                .next_back()
                .is_none_or(|c| !is_identifier_char(c))
        }) && line
            .get(end..)
            .is_some_and(|suffix| suffix.chars().next().is_none_or(|c| !is_identifier_char(c)))
    })
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn search_chunks(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let chunk_first = request.code_query_kind == CodeQueryKind::Hybrid
        && hybrid_query_prefers_chunk_first(request)
        && hybrid_query_should_use_layered_chunk_search(request);
    let chunk_candidate_limit = hybrid_chunk_candidate_limit(request);
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
            if hybrid_chunk_results_can_answer_without_graph_expansion(request, &hits)
                || hybrid_direct_results_can_answer_without_graph_expansion(request, &hits)
            {
                return Ok(hits);
            }
            narrow_hits =
                merge_strict_and_broad_chunk_hits(narrow_hits, hits, chunk_candidate_limit);
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
            chunk_candidate_limit,
        )?;
        retain_query_language_scoped_workflow_hits(request, &mut hits);
        narrow_hits = merge_strict_and_broad_chunk_hits(narrow_hits, hits, chunk_candidate_limit);
        if hybrid_chunk_results_can_answer_without_graph_expansion(request, &narrow_hits)
            || hybrid_direct_results_can_answer_without_graph_expansion(request, &narrow_hits)
        {
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
            chunk_candidate_limit,
        )?;
        retain_query_language_scoped_workflow_hits(request, &mut hits);
        narrow_hits = merge_strict_and_broad_chunk_hits(narrow_hits, hits, chunk_candidate_limit);
    }

    if chunk_first
        && let Some(compound_fts_query) = compound_hybrid_chunk_fts_match_query(&request.query)
    {
        let mut hits = search_chunks_with_fts_query(
            connection,
            status,
            request,
            &compound_fts_query,
            chunk_candidate_limit,
        )?;
        retain_query_language_scoped_workflow_hits(request, &mut hits);
        narrow_hits = merge_strict_and_broad_chunk_hits(narrow_hits, hits, chunk_candidate_limit);
    }
    if chunk_first
        && !narrow_hits.is_empty()
        && (hybrid_chunk_results_can_answer_without_graph_expansion(request, &narrow_hits)
            || hybrid_direct_results_can_answer_without_graph_expansion(request, &narrow_hits))
    {
        return Ok(narrow_hits);
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
        chunk_candidate_limit,
    )?;
    if !narrow_hits.is_empty() {
        hits = merge_strict_and_broad_chunk_hits(narrow_hits, hits, chunk_candidate_limit);
    }

    Ok(hits)
}

fn hybrid_chunk_candidate_limit(request: &CodeRetrievalRequest) -> usize {
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
    let sql = format!(
        "
        SELECT c.file_id, c.path, c.language_id, c.content, c.byte_start, c.byte_end,
               c.line_start, c.line_end, c.symbol_snapshot_id,
               symbol.canonical_symbol_id, symbol.name, symbol.qualified_name,
               f.parse_status, f.degraded_reason
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
                {fts_filter}
              ORDER BY bm25(code_repository_search) ASC, record_id ASC
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
            })
        },
    )?;
    let query = request.query.to_lowercase();
    let score_query = ScoreQuery::new(&request.query);
    let declaration_terms = query_terms(&query);
    let mut hits = Vec::new();
    for row in rows {
        let row = row.map_err(StorageError::from)?;
        if !selected_row(&row.path, &row.language_id, status, request) {
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
            + execution_flow_chunk_bonus(score, &request.query, &row.content, &row.path, request)
            + designated_initializer_chunk_bonus(
                score,
                &request.query,
                &row.content,
                &row.path,
                request,
            )
            + inline_construct_chunk_bonus(score, &request.query, &row.content, &row.path, request)
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
    let extra_filter = exact_language_filter_sql("language_id", query_language_filters.len());
    if extra_filter.is_empty() {
        return filter;
    }

    if filter.is_empty() {
        format!("AND {extra_filter}")
    } else {
        filter.push_str(" AND ");
        filter.push_str(&extra_filter);
        filter
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
    push_language_filter_values(&mut values, &status.language_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    push_language_filter_values(&mut values, query_language_filters);
    values.push(Value::Integer(fts_limit as i64));
    values.push(Value::Integer(limit as i64));

    values
}

fn strict_hybrid_chunk_candidate_limit(request: &CodeRetrievalRequest) -> usize {
    request
        .limit
        .max(1)
        .saturating_mul(STRICT_HYBRID_CHUNK_LIMIT_MULTIPLIER)
        .clamp(
            STRICT_HYBRID_CHUNK_MIN_CANDIDATES,
            STRICT_HYBRID_CHUNK_MAX_CANDIDATES,
        )
}

fn chunk_layers_for_request(
    request: &CodeRetrievalRequest,
    parse_status: &str,
) -> Vec<CodeRetrievalLayer> {
    let mut layers = chunk_layers(parse_status);
    if request.code_query_kind == CodeQueryKind::References
        && SymbolIdentityQuery::from_query(&request.query).is_some()
        && !layers.contains(&CodeRetrievalLayer::TextFallback)
    {
        layers.push(CodeRetrievalLayer::TextFallback);
    }

    layers
}

#[cfg(test)]
#[path = "code_query_unit_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "code_query_score_tests.rs"]
mod score_tests;

#[cfg(test)]
#[path = "code_query_identity_tests.rs"]
mod identity_tests;

#[cfg(test)]
#[path = "code_query_hybrid_symbol_planner_tests.rs"]
mod hybrid_symbol_planner_tests;

#[cfg(test)]
#[path = "code_query_hybrid_chunk_gate_tests.rs"]
mod hybrid_chunk_gate_tests;

#[cfg(test)]
#[path = "code_query_call_ranking_tests.rs"]
mod call_ranking_tests;

#[cfg(test)]
#[path = "code_query_indirect_call_tests.rs"]
mod indirect_call_tests;

#[cfg(test)]
#[path = "code_query_chunk_ranking_tests.rs"]
mod chunk_ranking_tests;

#[cfg(test)]
#[path = "code_query_symbol_ranking_tests.rs"]
mod symbol_ranking_tests;

#[cfg(test)]
#[path = "code_query_definition_fallback_tests.rs"]
mod definition_fallback_tests;

#[cfg(test)]
#[path = "code_query_reference_ranking_tests.rs"]
mod reference_ranking_tests;

#[cfg(test)]
#[path = "code_query_excerpt_tests.rs"]
mod excerpt_tests;
