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
) -> Result<(), StorageError> {
    maven::refresh_effective_dependencies_with_language_filters(
        transaction,
        source_scope,
        language_filters,
    )
}

pub(crate) fn rebuild_reference_search(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    super::search_documents::rebuild_reference_search_documents(transaction, source_scope)
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

// ---------------------------------------------------------------------------
// Path-aware variants — used by incremental sessions to avoid full-scope
// edge finalization when only a subset of paths changed.
// ---------------------------------------------------------------------------

pub(crate) fn resolve_references_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    affected_paths: &[&str],
) -> Result<(), StorageError> {
    references::normalize_unresolved_for_paths(transaction, source_scope, affected_paths)?;
    references::resolve_for_paths(transaction, source_scope, affected_paths)
}

pub(crate) fn resolve_imports_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    affected_paths: &[&str],
    symbol_cache: &mut FinalizeSymbolCache,
) -> Result<(), StorageError> {
    let file_languages = super::files::load_file_languages(transaction, source_scope)?;
    imports::resolve_for_paths(
        transaction,
        source_scope,
        &file_languages,
        affected_paths,
        &mut symbol_cache.symbols,
    )?;
    super::imported_references::resolve_references_for_paths(
        transaction,
        source_scope,
        affected_paths,
        &mut symbol_cache.symbols,
    )
}

pub(crate) fn resolve_call_targets_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    affected_paths: &[&str],
) -> Result<(), StorageError> {
    super::call_targets::resolve_references_for_paths(transaction, source_scope, affected_paths)
}

pub(crate) fn rebuild_reference_search_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    affected_paths: &[&str],
) -> Result<(), StorageError> {
    super::search_documents::rebuild_reference_search_documents_for_paths(
        transaction,
        source_scope,
        affected_paths,
    )
}

pub(crate) fn rebuild_calls_for_paths(
    transaction: &Transaction<'_>,
    source_scope: &str,
    repository_id: &str,
    affected_paths: &[&str],
    symbol_cache: &mut FinalizeSymbolCache,
) -> Result<(), StorageError> {
    calls::rebuild_for_paths(
        transaction,
        source_scope,
        repository_id,
        affected_paths,
        &mut symbol_cache.symbols,
    )
}
