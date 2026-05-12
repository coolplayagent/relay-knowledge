//! Pure domain model types.

mod code;
mod entity;
mod error;
mod graph_version;
mod index;
mod mutation;
mod retrieval;
mod source;

pub use code::{
    CodeChunkRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord, CodeGraphBatch,
    CodeGraphCommitReceipt, CodeParseStatus, CodeParseStatusCounts, CodeRange, CodeReferenceFields,
    CodeReferenceKind, CodeReferenceRecord, CodeResolutionState, CodeSymbolKind, CodeSymbolRecord,
};
pub use entity::KnowledgeEntity;
pub use error::DomainError;
pub use graph_version::GraphVersion;
pub use index::{IndexKind, IndexState, IndexStatus};
pub use mutation::{CommitReceipt, EvidenceRecord, GraphMutationBatch};
pub use retrieval::{FreshnessPolicy, RetrievalHit, RetrievalMode};
pub use source::SourceScope;
