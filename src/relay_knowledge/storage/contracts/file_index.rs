//! Compatibility surface for file-index contracts moved into the domain layer.

pub use crate::domain::{
    FileContentChunk, FileContentEntry, FileContentReadModelCursor, FileContentSearchHit,
    FileContentSearchRequest, FileIndexDiagnostics, FileIndexEntry, FileIndexRoot,
    FileIndexRootStatus, FileIndexRootUpdate, FileIndexScanSummary, FileKnowledgeFactCandidate,
    FileSearchHit, FileSearchRequest,
};
