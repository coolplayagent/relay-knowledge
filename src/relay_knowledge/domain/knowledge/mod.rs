mod file_index;
mod map;

use super::core::{DomainError, SourceScope, error};

pub use file_index::{
    FileContentChunk, FileContentEntry, FileContentReadModelCursor, FileContentSearchHit,
    FileContentSearchRequest, FileIndexDiagnostics, FileIndexEntry, FileIndexRoot,
    FileIndexRootStatus, FileIndexRootUpdate, FileIndexScanSummary, FileKnowledgeFactCandidate,
    FileSearchHit, FileSearchRequest,
};
pub use map::{
    KnowledgeMap, KnowledgeMapChange, KnowledgeMapHistoryEntry, KnowledgeMapRoute,
    KnowledgeMapSource, KnowledgeMapSourceKind, KnowledgeMapTopic,
};
