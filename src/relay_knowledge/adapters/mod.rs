//! Concrete implementations of technology-neutral application ports.

mod embedding;
mod release_metadata;
mod storage;
mod worker_outbound;

pub use embedding::NetworkEmbeddingProvider;
pub use storage::SqliteKnowledgeStoreFactory;
pub use worker_outbound::NetworkWorkerOutbound;
