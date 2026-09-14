//! Defines observable finalization phase states and their bounded stage entry points.

use rusqlite::Transaction;

use super::{calls, imports, references, symbols};
use crate::storage::{StorageError, sqlite::maven};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(crate) const RESOLVE_REFERENCES: &str = "finalizing:resolve_references";
pub(crate) const BUILD_QUERY_INDEXES: &str = "finalizing:build_query_indexes";
pub(crate) const RESOLVE_IMPORTS: &str = "finalizing:resolve_imports";
pub(crate) const RESOLVE_CALL_TARGETS: &str = "finalizing:resolve_call_targets";
pub(crate) const REFRESH_DEPENDENCIES: &str = "finalizing:refresh_dependencies";
pub(crate) const REBUILD_REFERENCE_SEARCH: &str = "finalizing:rebuild_reference_search";
pub(crate) const REBUILD_CALLS: &str = "finalizing:rebuild_calls";
pub(crate) const RESOLVE_WORKSPACE_IMPORTS: &str = "finalizing:resolve_workspace_imports";
pub(crate) const PUBLISH_SCOPE: &str = "finalizing:publish_scope";
pub(crate) const SOFTWARE_PROJECTION: &str = crate::domain::SOFTWARE_PROJECTION_CHECKPOINT;
pub(crate) const PARTITIONED_PUBLISH: &str = "finalizing:partitioned_publish";

pub(crate) const ORDERED_FINALIZATION_PHASES: [&str; 11] = [
    BUILD_QUERY_INDEXES,
    RESOLVE_REFERENCES,
    RESOLVE_IMPORTS,
    RESOLVE_CALL_TARGETS,
    REFRESH_DEPENDENCIES,
    REBUILD_REFERENCE_SEARCH,
    REBUILD_CALLS,
    PUBLISH_SCOPE,
    RESOLVE_WORKSPACE_IMPORTS,
    SOFTWARE_PROJECTION,
    PARTITIONED_PUBLISH,
];

const _: [(); crate::storage::CODE_INDEX_FINALIZATION_COARSE_PHASE_COUNT] =
    [(); ORDERED_FINALIZATION_PHASES.len()];

pub(crate) fn position(state: &str) -> Option<usize> {
    ORDERED_FINALIZATION_PHASES
        .iter()
        .position(|phase| *phase == state)
}

#[derive(Default)]
pub(crate) struct FinalizeSymbolCache {
    symbols: Option<Vec<symbols::SymbolKey>>,
}

pub(crate) fn resolve_references(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    references::normalize_unresolved(transaction, source_scope)?;
    references::resolve(transaction, source_scope)
}

pub(crate) fn resolve_imports(
    transaction: &Transaction<'_>,
    source_scope: &str,
    symbol_cache: &mut FinalizeSymbolCache,
) -> Result<(), StorageError> {
    let file_languages = super::files::load_file_languages(transaction, source_scope)?;
    imports::resolve(
        transaction,
        source_scope,
        &file_languages,
        &mut symbol_cache.symbols,
    )?;
    super::imported_references::resolve_references(
        transaction,
        source_scope,
        &mut symbol_cache.symbols,
    )
}

pub(crate) fn resolve_call_targets(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    super::call_targets::resolve_references(transaction, source_scope)
}

pub(crate) fn refresh_dependencies(
    transaction: &Transaction<'_>,
    source_scope: &str,
    language_filters: &[String],
) -> Result<maven::EffectiveDependencyRefresh, StorageError> {
    maven::refresh_effective_dependencies_with_language_filters(
        transaction,
        source_scope,
        language_filters,
    )
}

pub(crate) fn rebuild_reference_search(
    transaction: &Transaction<'_>,
    source_scope: &str,
    resource_budget: crate::domain::CodeIndexResourceBudget,
    expected_reference_count: usize,
) -> Result<(), StorageError> {
    super::search_documents::rebuild_reference_search_documents(
        transaction,
        source_scope,
        resource_budget,
        expected_reference_count,
    )
}

pub(crate) fn rebuild_calls(
    transaction: &Transaction<'_>,
    source_scope: &str,
    repository_id: &str,
    symbol_cache: &mut FinalizeSymbolCache,
) -> Result<(), StorageError> {
    calls::rebuild(
        transaction,
        source_scope,
        repository_id,
        &mut symbol_cache.symbols,
    )
}
