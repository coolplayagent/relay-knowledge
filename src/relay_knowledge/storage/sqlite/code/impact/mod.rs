//! Bounded SQLite code-impact orchestration over focused evidence owners.

mod evidence;
mod evidence_budget;
mod path_selection;
mod reactor;
mod seed;

#[cfg(test)]
mod evidence_test_support;
#[cfg(test)]
mod evidence_tests;
#[cfg(test)]
#[path = "evidence_budget_tests.rs"]
mod impact_evidence_budget_suite;
#[cfg(test)]
mod path_selection_tests;
#[cfg(test)]
mod seed_tests;

use rusqlite::Connection;

use crate::{
    domain::{CodeImpactRequest, CodeRepositoryStatus, CodeRetrievalHit, CodeRetrievalLayer},
    storage::{CodeImpactChanges, StorageError},
};

use self::{
    evidence::{callers_for_symbols, chunks_for_paths, importers_for_modules},
    evidence_budget::ImpactHits,
    path_selection::selected_changed_paths,
    seed::{import_module_seeds, symbol_seeds_for_paths},
};
use super::query::{required_repository, required_scope};

pub(super) fn analyze_impact(
    connection: &mut Connection,
    request: CodeImpactRequest,
    changes: CodeImpactChanges,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    analyze_impact_with_status(connection, &status, request, changes)
}

pub(super) fn analyze_impact_scope(
    connection: &mut Connection,
    source_scope: &str,
    request: CodeImpactRequest,
    changes: CodeImpactChanges,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let status = super::status::repository_scope_status_by_source_scope(connection, source_scope)?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository source scope '{source_scope}' is not indexed"
            ))
        })?;

    analyze_impact_with_status(connection, &status, request, changes)
}

fn analyze_impact_with_status(
    connection: &mut Connection,
    status: &CodeRepositoryStatus,
    request: CodeImpactRequest,
    changes: CodeImpactChanges,
) -> Result<Vec<CodeRetrievalHit>, StorageError> {
    let changed = selected_changed_paths(connection, status, &request, changes.paths)?;
    let changed_symbols = symbol_seeds_for_paths(connection, required_scope(status)?, &changed)?;
    let changed_modules =
        import_module_seeds(&changed, &changed_symbols, &changes.deleted_symbol_names);
    // Feed score classes from highest to lowest so lower-scored duplicate
    // evidence can still enrich the retained higher-scored hit's provenance.
    let mut hits = ImpactHits::new(request.limit);
    for hit in reactor::module_impacts(connection, status, &request, &changed)? {
        hits.push(hit);
    }
    chunks_for_paths(connection, status, &changed, &request, &mut hits)?;
    callers_for_symbols(
        connection,
        status,
        &changed_symbols.symbol_ids,
        &changes.deleted_symbol_names,
        &request,
        &mut hits,
    )?;
    importers_for_modules(connection, status, &changed_modules, &request, &mut hits)?;
    let mut hits = hits.finish();
    for hit in &mut hits {
        hit.retrieval_layers.push(CodeRetrievalLayer::Impact);
        hit.score += 3.0;
    }

    Ok(hits)
}
