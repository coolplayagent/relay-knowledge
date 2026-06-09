use rusqlite::Transaction;

use crate::storage::StorageError;

pub(crate) const RESOLVE_REFERENCES: &str = "finalizing:resolve_references";
pub(crate) const RESOLVE_IMPORTS: &str = "finalizing:resolve_imports";
pub(crate) const RESOLVE_CALL_TARGETS: &str = "finalizing:resolve_call_targets";
pub(crate) const REFRESH_DEPENDENCIES: &str = "finalizing:refresh_dependencies";
pub(crate) const REBUILD_REFERENCE_SEARCH: &str = "finalizing:rebuild_reference_search";
pub(crate) const REBUILD_CALLS: &str = "finalizing:rebuild_calls";
pub(crate) const RESOLVE_WORKSPACE_IMPORTS: &str = "finalizing:resolve_workspace_imports";
pub(crate) const PUBLISH_SCOPE: &str = "finalizing:publish_scope";

#[derive(Default)]
pub(crate) struct FinalizeSymbolCache {
    symbols: Option<Vec<super::SymbolKey>>,
}

pub(crate) fn resolve_references(
    transaction: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    super::normalize_unresolved_references(transaction, source_scope)?;
    super::resolve_references(transaction, source_scope)
}

pub(crate) fn resolve_imports(
    transaction: &Transaction<'_>,
    source_scope: &str,
    symbol_cache: &mut FinalizeSymbolCache,
) -> Result<(), StorageError> {
    let file_languages = super::files::load_file_languages(transaction, source_scope)?;
    super::resolve_imports(
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
    super::maven::refresh_effective_dependencies_with_language_filters(
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
    super::rebuild_calls(
        transaction,
        source_scope,
        repository_id,
        &mut symbol_cache.symbols,
    )
}
