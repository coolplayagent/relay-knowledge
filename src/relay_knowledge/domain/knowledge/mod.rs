mod file_index;
mod map;
mod map_directory;

use super::core::{DomainError, SourceScope, error};

pub use file_index::{
    FileContentChunk, FileContentEntry, FileContentReadModelCursor, FileContentSearchHit,
    FileContentSearchRequest, FileIndexDiagnostics, FileIndexEntry, FileIndexRoot,
    FileIndexRootStatus, FileIndexRootUpdate, FileIndexScanSummary, FileKnowledgeFactCandidate,
    FileSearchHit, FileSearchRequest,
};
pub(crate) use map::{BUSINESS_GLOSSARY_RELATIVE_PATH, LEGACY_BUSINESS_GLOSSARY_RELATIVE_PATH};
pub use map::{
    KnowledgeMap, KnowledgeMapChange, KnowledgeMapHistoryEntry, KnowledgeMapRoute,
    KnowledgeMapSource, KnowledgeMapSourceKind, KnowledgeMapTopic,
};
pub(crate) use map_directory::validate_directory_collection;
pub use map_directory::{
    DirectoryLoadHint, DirectoryRelation, DirectoryRelationKind, DirectoryUpdateRule,
    RepositoryMapDirectory, RepositoryMapDirectoryChange, RepositoryMapType,
};
