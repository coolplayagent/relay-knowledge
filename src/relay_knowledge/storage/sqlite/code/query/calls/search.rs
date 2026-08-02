use rusqlite::Connection;

use crate::{
    domain::{CodeQueryKind, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest},
    storage::StorageError,
};

use super::super::{
    filter_dedupe_sort_truncate, has_query_field_hit_filters, query_field_filtered_hits_for_gate,
};
use super::{
    ambiguous_callees::search_ambiguous_callee_implementation_hits,
    hit_projection::call_rows_to_hits,
    identity_query::{
        call_identity_hits_can_answer_without_fts, call_identity_leaf_or_selector_is_specific,
        call_identity_query,
    },
    indirect::search_indirect_call_identity_rows,
    row_store::{search_call_fts_rows, search_call_identity_rows},
};

pub(in super::super) fn search_calls(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let identity = call_identity_query(request);
    let mut identity_hits = Vec::new();
    if let Some(identity) = &identity {
        let identity_rows = search_call_identity_rows(connection, status, request, identity)?;
        let saturated = identity_rows.saturated;
        let rows = identity_rows
            .rows
            .into_iter()
            .filter(|row| identity.matches_row(row))
            .collect::<Vec<_>>();
        let direct_hit_count = rows.len();
        let implementation_hits =
            search_ambiguous_callee_implementation_hits(connection, status, request, &rows)?;
        identity_hits = call_rows_to_hits(status, request, rows);
        identity_hits.extend(implementation_hits);
        let mut saturated = saturated;
        if request.code_query_kind == CodeQueryKind::Callers
            && !saturated
            && (direct_hit_count == 0
                || call_identity_leaf_or_selector_is_specific(request, identity))
        {
            let indirect_rows =
                search_indirect_call_identity_rows(connection, status, request, identity)?;
            saturated = saturated || indirect_rows.saturated;
            identity_hits.extend(call_rows_to_hits(status, request, indirect_rows.rows));
        }
        let filtered_identity_hits = has_query_field_hit_filters(request)
            .then(|| query_field_filtered_hits_for_gate(&identity_hits, request));
        let identity_gate_hit_count = filtered_identity_hits
            .as_ref()
            .map_or(identity_hits.len(), Vec::len);
        if call_identity_hits_can_answer_without_fts(
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
    }

    let mut hits = call_rows_to_hits(
        status,
        request,
        search_call_fts_rows(connection, status, request)?,
    );
    hits.extend(identity_hits);

    Ok(hits)
}
