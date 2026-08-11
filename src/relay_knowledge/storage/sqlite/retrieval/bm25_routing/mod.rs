//! Bounded, content-driven coarse routing for the global BM25 read model.

mod persistence;
mod selection;
mod terms;

pub(super) use persistence::{
    RebuildLease, begin_rebuild, checkpoint_rebuild, code_document_batch, configure_rebuild,
    delete_code_document_batch, delete_document, finish_rebuild, mark_label_gram_state,
    renew_rebuild, replace_document,
};
pub(in crate::storage::sqlite) use persistence::{ensure_rebuild_inactive, mark_graph_version};
pub(super) use selection::{Bm25RoutingPlan, plan_query};

pub(super) const ROUTING_ALGORITHM_VERSION: &str =
    "simhash10-topical4-indexed-scope64-partition-ascii-subset128b-256t-a1-docidlen1-v4";

pub(super) struct Bm25RoutingText<'a> {
    pub(super) source_scope: &'a str,
    pub(super) source_path: Option<&'a str>,
    pub(super) entity_labels: &'a str,
    pub(super) entity_aliases: &'a str,
    pub(super) content: &'a str,
    pub(super) graph_version: u64,
}

pub(super) struct PreparedBm25Route {
    source_scope: String,
    pub(in crate::storage::sqlite::retrieval) routing_key: String,
    pub(in crate::storage::sqlite::retrieval) group_token: String,
    graph_version: u64,
    term_counts: Vec<(String, u32)>,
}

pub(in crate::storage::sqlite::retrieval) fn scope_token(source_scope: &str) -> String {
    let scope_hash = super::local_model::stable_hash64(source_scope.as_bytes());
    format!("rks{scope_hash:016x}")
}

pub(super) fn prepare_document(input: Bm25RoutingText<'_>) -> PreparedBm25Route {
    let topical = terms::topical_inventory(&input);
    let all_fields = terms::indexed_inventory(&input);
    let scope_token = scope_token(input.source_scope);
    let scope_hash = super::local_model::stable_hash64(input.source_scope.as_bytes());
    let group = terms::simhash_prefix(&topical, 10);
    let group_token = format!("rkg{scope_hash:016x}{group:03x}");
    let routing_key = format!("{scope_token} {group_token}");

    PreparedBm25Route {
        source_scope: input.source_scope.to_owned(),
        routing_key,
        group_token,
        graph_version: input.graph_version,
        term_counts: all_fields.counts,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
