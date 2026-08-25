use rusqlite::Connection;
#[cfg(test)]
use rusqlite::types::Value;

mod api_identities;
mod calls;
mod chunks;
mod conversion_terms;
mod excerpts;
pub(super) mod hits;
mod hybrid;
mod identifiers;
mod imports;
mod line_ranges;
pub(super) mod prepare;
mod references;
mod relevance;
mod routes;
mod rows;
mod sbom;
mod scoring;
mod symbols;

#[cfg(test)]
use crate::domain::{CodeRetrievalLayer, RepositoryCodeRange};
use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest},
    storage::StorageError,
};

#[cfg(test)]
const MAX_CANDIDATE_BIND_VALUES: usize = 900;

pub(super) use self::hits::{
    HitParts, chunk_layers, dedupe_sort_truncate, filter_dedupe_sort_truncate,
    filtered_hits_for_gate, has_query_field_hit_filters, hit_from_parts, mark_hits_degraded,
    query_field_filtered_hits_for_gate, required_repository, required_scope, selected_row,
};
use self::prepare::{
    code_search_error_can_use_empty_results, code_search_plannable_outage_reason,
    code_search_read_model_unavailable_reason, prepare_code_search_statement,
    retry_code_search_operation,
};
pub(super) use super::code_query_scope::{language_filter_allows, path_filter_allows};
use calls::search_calls;
use chunks::canonical_symbol_leaf_matches;
use chunks::{
    definition_query_needs_chunk_fallback, references_query_needs_chunk_fallback, search_chunks,
};
use hybrid::chunk_gate::{
    hybrid_hits_can_answer_without_graph_expansion, retain_query_language_scoped_workflow_hits,
};
use hybrid::exact_path::{
    hybrid_exact_path_query_can_defer_to_source_fallback, hybrid_query_can_skip_graph_expansion,
    hybrid_query_should_use_layered_chunk_search,
};
use hybrid::planning::hybrid_query_prefers_chunk_first;
#[cfg(test)]
use hybrid::planning::query_language_scoped_workflow_surface_scopes;
#[cfg(test)]
use imports::path_context::import_target_symbol_query;
#[cfg(test)]
use imports::scoring::{
    import_public_dependency_surface_bonus, import_reexport_surface_penalty,
    import_self_implementation_penalty, import_source_path_query_overlap_bonus,
    import_surface_bonus, import_target_symbol_bonus,
};
use imports::search_imports;
use references::search_references;
use relevance::*;
use routes::search_routes;
use sbom::search_sbom;
use symbols::{hybrid_symbol_query_can_answer_without_non_symbol_layers, search_symbols};

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
    let status = super::status::repository_scope_status_by_source_scope(connection, source_scope)?
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
                let mut filtered_chunk_hits = filtered_hits_for_gate(&chunk_hits, request);
                if hybrid_hits_can_answer_without_graph_expansion(request, &filtered_chunk_hits) {
                    if let Some(partial_hits) = append_hits_or_return_partial_on_search_outage(
                        &mut filtered_chunk_hits,
                        request,
                        search_routes(connection, status, request),
                    )? {
                        return Ok(partial_hits);
                    }
                    hits.extend(filtered_chunk_hits);
                    filter_dedupe_sort_truncate(&mut hits, request);
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
        if request.code_query_kind == CodeQueryKind::Hybrid {
            let route_hits = search_routes(connection, status, request);
            if let Some(partial_hits) =
                append_hits_or_return_partial_on_search_outage(&mut hits, request, route_hits)?
            {
                return Ok(partial_hits);
            }
        }
        if hybrid_symbol_query_can_answer_without_non_symbol_layers(request, &hits) {
            filter_dedupe_sort_truncate(&mut hits, request);
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
        let filtered_hits = filtered_hits_for_gate(&hits, request);
        if hybrid_exact_path_query_can_defer_to_source_fallback(request, &filtered_hits) {
            return Ok(filtered_hits);
        }
        if !searched_chunks {
            let chunk_hits = search_chunks(connection, status, request);
            if let Some(partial_hits) =
                append_hits_or_return_partial_on_search_outage(&mut hits, request, chunk_hits)?
            {
                return Ok(partial_hits);
            }
        }
        let filtered_hits = filtered_hits_for_gate(&hits, request);
        if hybrid_hits_can_answer_without_graph_expansion(request, &filtered_hits) {
            filter_dedupe_sort_truncate(&mut hits, request);
            return Ok(hits);
        }
        if hybrid_query_can_skip_graph_expansion(request, &hits) {
            filter_dedupe_sort_truncate(&mut hits, request);
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
        filter_dedupe_sort_truncate(&mut hits, request);
        return Ok(hits);
    }
    if request.code_query_kind == CodeQueryKind::References {
        hits.extend(search_references(connection, status, request)?);
    }
    if references_query_needs_chunk_fallback(request, &hits) {
        let chunk_hits = search_chunks(connection, status, request);
        if let Some(partial_hits) =
            append_hits_or_return_partial_on_search_outage(&mut hits, request, chunk_hits)?
        {
            return Ok(partial_hits);
        }
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
    filter_dedupe_sort_truncate(&mut hits, request);

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
        Err(error) => {
            let Some(reason) = code_search_plannable_outage_reason(request, &error)
                .or_else(|| hybrid_chunk_first_search_outage_reason(request, &error))
            else {
                return Err(error);
            };
            let filtered_hits = filtered_hits_for_gate(hits, request);
            if filtered_hits.is_empty() {
                return Err(error);
            }
            *hits = filtered_hits;
            mark_hits_degraded(hits, &reason);
            filter_dedupe_sort_truncate(hits, request);
            Ok(Some(std::mem::take(hits)))
        }
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

#[cfg(test)]
#[path = "tests/mod.rs"]
mod test_modules;
