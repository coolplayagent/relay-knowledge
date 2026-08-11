mod advanced;
mod aliases;
mod bm25;
mod bm25_fallback;
mod bm25_routing;
mod context;
mod derived;
mod label_trigrams;
mod local_model;
mod ranking;
mod read_model;

use bm25::RawBm25Row;

pub(in crate::storage::sqlite) use bm25_routing::{
    ensure_rebuild_inactive as ensure_bm25_rebuild_inactive,
    mark_graph_version as mark_bm25_route_graph_version,
};

#[cfg(test)]
pub(super) use read_model::initialize_schema;
pub(super) use read_model::{
    Bm25WriteTarget, EvidenceDocumentInput, RetrievalWriteContext, delete_code_documents,
    derived_documents_current, initialize_schema_with_generation_finalizer,
    insert_code_chunk_document, insert_code_symbol_document, replace_evidence_document,
    search_graph,
};

use read_model::{
    ScoredHit, evidence_group_key, graph_bm25_transient_error_message, parse_f64_array,
    parse_string_array, scored_bm25_hit, sort_scored_hits, split_labels,
};

#[cfg(test)]
pub(super) const LOCAL_TOKENIZER_VERSION: &str = read_model::LOCAL_TOKENIZER_VERSION;
