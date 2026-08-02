use rusqlite::Connection;

use crate::storage::StorageError;

use super::label_trigrams;

mod bm25_hit;
mod candidate;
mod documents;
mod migration;
mod schema;
mod search;

pub(in crate::storage::sqlite::retrieval) use bm25_hit::scored_bm25_hit;
pub(in crate::storage::sqlite::retrieval) use candidate::{
    ScoredHit, evidence_group_key, sort_scored_hits,
};
#[cfg(test)]
pub(in crate::storage::sqlite) use documents::LOCAL_TOKENIZER_VERSION;
pub(in crate::storage::sqlite) use documents::{
    EvidenceDocumentInput, delete_code_documents, insert_code_chunk_document,
    insert_code_symbol_document, replace_evidence_document,
};
pub(in crate::storage::sqlite::retrieval) use documents::{
    parse_f64_array, parse_string_array, split_labels,
};
pub(in crate::storage::sqlite::retrieval) use schema::graph_bm25_transient_error_message;
pub(in crate::storage::sqlite) use search::search_graph;

pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    schema::execute_retrieval_schema(connection)?;
    label_trigrams::initialize_schema(connection)?;
    if migration::derived_documents_missing(connection)? {
        migration::rebuild_bm25_documents(connection)?;
    }
    label_trigrams::backfill_missing(connection)?;
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
