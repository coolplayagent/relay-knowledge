use rusqlite::Connection;

use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest},
    storage::StorageError,
};

use super::{
    fts::search_symbol_fts_rows,
    hybrid_symbol_direct,
    identity::{
        api_identity_rows_can_answer_without_fts, identity_hits_can_answer_without_fts,
        identity_miss_can_answer_without_fts, search_hybrid_api_identity_rows,
        search_symbol_identity_rows, symbol_identity_name_exists,
    },
    ranking::symbol_rows_to_hits,
};
use crate::storage::sqlite::code::query::{
    api_identities::hybrid_api_symbol_identities, code_search_plannable_outage_reason,
    code_search_read_model_unavailable_reason, filter_dedupe_sort_truncate,
    has_query_field_hit_filters, hybrid::planning::hybrid_query_prefers_chunk_first,
    mark_hits_degraded, query_field_filtered_hits_for_gate, relevance::SymbolIdentityQuery,
};

pub(in crate::storage::sqlite::code::query) fn search_symbols(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
    direct_symbol_recovery_reason: Option<&str>,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let identity = SymbolIdentityQuery::from_query(&request.query);
    let api_identities = hybrid_api_symbol_identities(&request.query, request);
    let mut identity_hits = Vec::new();
    if let Some(identity) = &identity {
        if identity_miss_can_answer_without_fts(request, false, identity)
            && !symbol_identity_name_exists(connection, status, request, identity.leaf_name())?
        {
            return Ok(Vec::new());
        }
        let identity_rows = search_symbol_identity_rows(connection, status, request, identity)?;
        let saturated = identity_rows.saturated;
        let rows = identity_rows
            .rows
            .into_iter()
            .filter(|row| {
                identity.matches_symbol(
                    &row.name,
                    &row.qualified_name,
                    &row.signature,
                    &row.canonical_symbol_id,
                )
            })
            .collect::<Vec<_>>();
        identity_hits = symbol_rows_to_hits(status, request, rows, &api_identities);
        let filtered_identity_hits = has_query_field_hit_filters(request)
            .then(|| query_field_filtered_hits_for_gate(&identity_hits, request));
        let identity_gate_hit_count = filtered_identity_hits
            .as_ref()
            .map_or(identity_hits.len(), Vec::len);
        if identity_hits_can_answer_without_fts(
            request,
            identity,
            identity_gate_hit_count,
            saturated,
        ) {
            if let Some(mut hits) = filtered_identity_hits {
                hits.truncate(request.limit);
                return Ok(hits);
            }
            filter_dedupe_sort_truncate(&mut identity_hits, request);
            return Ok(identity_hits);
        }
        let field_filters_removed_identity_hits =
            identity_gate_hit_count == 0 && !identity_hits.is_empty();
        if identity_miss_can_answer_without_fts(request, saturated, identity)
            && !field_filters_removed_identity_hits
        {
            return Ok(Vec::new());
        }
    }

    let api_identity_rows =
        search_hybrid_api_identity_rows(connection, status, request, &api_identities)?;
    let api_identity_can_answer =
        api_identity_rows_can_answer_without_fts(request, &api_identities, &api_identity_rows);
    let api_identity_hits =
        symbol_rows_to_hits(status, request, api_identity_rows.rows, &api_identities);
    if api_identity_can_answer {
        let filtered_api_hits = has_query_field_hit_filters(request)
            .then(|| query_field_filtered_hits_for_gate(&api_identity_hits, request));
        if let Some(mut hits) = filtered_api_hits {
            if hits.is_empty() {
                identity_hits.extend(api_identity_hits);
            } else {
                hits.truncate(request.limit);
                return Ok(hits);
            }
        } else {
            let mut hits = api_identity_hits;
            filter_dedupe_sort_truncate(&mut hits, request);
            return Ok(hits);
        }
    } else {
        identity_hits.extend(api_identity_hits);
    }
    if let Some(reason) = direct_symbol_recovery_reason
        && let Some(mut hits) = hybrid_symbol_direct::search_hybrid_direct_symbol_hits(
            connection,
            status,
            request,
            &api_identities,
        )?
    {
        mark_hits_degraded(&mut hits, reason);
        return Ok(hits);
    }
    let symbol_fts_rows = match search_symbol_fts_rows(connection, status, request) {
        Ok(rows) => rows,
        Err(error) => {
            let Some(reason) = code_search_plannable_outage_reason(request, &error)
                .or_else(|| hybrid_chunk_first_symbol_outage_reason(request, &error))
            else {
                return Err(error);
            };
            if let Some(mut hits) = hybrid_symbol_direct::search_hybrid_direct_symbol_hits(
                connection,
                status,
                request,
                &api_identities,
            )? {
                mark_hits_degraded(&mut hits, &reason);
                return Ok(hits);
            }
            let mut hits = identity_hits;
            if hits.is_empty() {
                return Err(error);
            }
            mark_hits_degraded(&mut hits, &reason);
            filter_dedupe_sort_truncate(&mut hits, request);
            return Ok(hits);
        }
    };
    let mut hits = symbol_rows_to_hits(status, request, symbol_fts_rows, &api_identities);
    hits.extend(identity_hits);

    Ok(hits)
}

fn hybrid_chunk_first_symbol_outage_reason(
    request: &CodeRetrievalRequest,
    error: &StorageError,
) -> Option<String> {
    (request.code_query_kind == CodeQueryKind::Hybrid && hybrid_query_prefers_chunk_first(request))
        .then(|| code_search_read_model_unavailable_reason(error))
        .flatten()
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
