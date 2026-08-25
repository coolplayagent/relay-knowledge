//! Bounded import retrieval orchestration across direct, target, and FTS layers.

use rusqlite::Connection;

use crate::{
    domain::{CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalRequest},
    storage::StorageError,
};

use super::code_search_read_model_unavailable_reason;

pub(super) mod binding_terms;
mod hit_projection;
pub(super) mod path_context;
mod row_store;
pub(super) mod scoring;
pub(super) mod targets;

use self::{
    hit_projection::import_rows_to_hits,
    row_store::{
        import_path_rows_can_answer_without_fts, import_path_rows_fit_request,
        import_target_symbol_rows_can_answer_without_fts, search_import_fts_rows,
        search_import_identifier_rows, search_import_path_rows,
    },
    targets::search_imports_by_target_symbols,
};

const IMPORT_EXACT_EDGE_RESERVE_LIMIT: usize = 700;

pub(super) fn search_imports(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    request: &CodeRetrievalRequest,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let direct_rows = search_import_path_rows(connection, status, request)?;
    let direct_rows_can_answer = import_path_rows_can_answer_without_fts(request, &direct_rows);
    if direct_rows_can_answer && import_path_rows_fit_request(request, &direct_rows) {
        return import_rows_to_hits(connection, status, request, direct_rows.rows);
    }

    let identifier_rows = search_import_identifier_rows(connection, status, request)?;
    let target_symbol_rows = match search_imports_by_target_symbols(connection, status, request) {
        Ok(rows) => rows,
        Err(error) if code_search_read_model_unavailable_reason(&error).is_some() => Vec::new(),
        Err(error) => return Err(error),
    };
    let target_symbol_rows_can_answer =
        import_target_symbol_rows_can_answer_without_fts(request, &target_symbol_rows);
    if target_symbol_rows_can_answer && identifier_rows.is_empty() {
        return import_rows_to_hits(connection, status, request, target_symbol_rows);
    }

    match search_import_fts_rows(connection, status, request) {
        Ok(mut rows) => {
            rows.extend(direct_rows.rows);
            rows.extend(target_symbol_rows);
            rows.extend(identifier_rows);
            import_rows_to_hits(connection, status, request, rows)
        }
        Err(error) if code_search_read_model_unavailable_reason(&error).is_some() => {
            let mut rows = direct_rows.rows;
            rows.extend(target_symbol_rows);
            rows.extend(identifier_rows);
            if rows.is_empty() {
                Err(error)
            } else {
                import_rows_to_hits(connection, status, request, rows)
            }
        }
        Err(_) if direct_rows_can_answer => {
            import_rows_to_hits(connection, status, request, direct_rows.rows)
        }
        Err(_) if target_symbol_rows_can_answer => {
            import_rows_to_hits(connection, status, request, target_symbol_rows)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod target_tests;

#[cfg(test)]
#[path = "generated_tests.rs"]
mod generated_tests;

#[cfg(test)]
#[path = "ranking_tests.rs"]
mod ranking_tests;

#[cfg(test)]
#[path = "foundational_ranking_tests.rs"]
mod foundational_ranking_tests;

#[cfg(test)]
#[path = "importer_significance_tests.rs"]
mod importer_significance_tests;

#[cfg(test)]
#[path = "usage_ranking_tests.rs"]
mod usage_ranking_tests;

#[cfg(test)]
#[path = "outage_tests.rs"]
mod outage_tests;
