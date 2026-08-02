//! SQLite storage adapter.
//!
//! The root composes schema, runtime, graph, indexing, code, and operational
//! owners while keeping concrete connection lifecycle and transactions in
//! responsibility-named modules.

mod canvas;
mod code;
mod code_graph;
mod connection_runtime;
mod evidence_identity;
mod file_index;
mod graph;
mod indexing;
mod maven;
mod mutation_log;
mod operations;
mod retrieval;
mod schema;
mod scope_filters;
mod software;
mod store;
mod table_stats;

pub(in crate::storage) use connection_runtime::maintenance::{
    configure_connection, read_only_database_diagnostics,
};
pub use store::SqliteGraphStore;

#[cfg(test)]
use crate::{
    domain::{GraphMutationBatch, GraphVersion, IndexKind, RetrievalHit},
    storage::{CodeGraphStore, GraphSearchRequest, GraphStore, IndexStore, MutationLogStore},
};

#[cfg(test)]
#[path = "tests/metadata.rs"]
mod metadata_tests;

#[cfg(test)]
#[path = "tests/graph_storage/mod.rs"]
mod graph_storage_tests;

#[cfg(test)]
#[path = "tests/graphrag_phase4.rs"]
mod graphrag_phase4_tests;
