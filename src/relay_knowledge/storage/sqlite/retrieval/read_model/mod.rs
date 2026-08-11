use rusqlite::Connection;

use crate::storage::StorageError;

use super::label_trigrams;

mod bm25_hit;
mod candidate;
mod documents;
mod identity;
mod migration;
mod rebuild_budget;
mod schema;
mod search;

pub(in crate::storage::sqlite::retrieval) use bm25_hit::scored_bm25_hit;
pub(in crate::storage::sqlite::retrieval) use candidate::{
    ScoredHit, evidence_group_key, sort_scored_hits,
};
#[cfg(test)]
pub(in crate::storage::sqlite) use documents::LOCAL_TOKENIZER_VERSION;
pub(in crate::storage::sqlite) use documents::{
    Bm25WriteTarget, EvidenceDocumentInput, RetrievalWriteContext, delete_code_documents,
    insert_code_chunk_document, insert_code_symbol_document, replace_evidence_document,
};
pub(in crate::storage::sqlite::retrieval) use documents::{
    parse_f64_array, parse_string_array, split_labels,
};
pub(in crate::storage::sqlite::retrieval) use schema::graph_bm25_transient_error_message;
pub(in crate::storage::sqlite) use search::search_graph;

#[cfg(test)]
pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    initialize_schema_with_generation_finalizer(connection, |_| Ok(()))
}

pub(in crate::storage::sqlite) fn initialize_schema_with_generation_finalizer<F>(
    connection: &Connection,
    finalize_generation: F,
) -> Result<(), StorageError>
where
    F: FnOnce(&Connection) -> Result<(), StorageError>,
{
    schema::execute_retrieval_schema(connection)?;
    label_trigrams::initialize_schema(connection)?;
    let rebuilt_retrieval_generation = migration::derived_documents_missing(connection)?;
    if rebuilt_retrieval_generation {
        migration::rebuild_bm25_documents(connection, finalize_generation)?;
    }
    if !rebuilt_retrieval_generation {
        label_trigrams::backfill_missing(connection)?;
    }
    migration::cleanup_retired_bm25_generation_if_current(connection)?;
    Ok(())
}

pub(in crate::storage::sqlite) fn derived_documents_current(
    connection: &Connection,
) -> Result<bool, StorageError> {
    Ok(!migration::derived_documents_missing(connection)?)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod mod_tests;
