use crate::domain::{
    CodeChunkRecord, CodeGraphBatch, CodeGraphCommitReceipt, CodeReferenceRecord, CodeSymbolRecord,
    GraphVersion,
};

use super::StorageFuture;

/// Code graph fact persistence and query contract for tree-sitter output.
pub trait CodeGraphStore: Send + Sync {
    fn commit_code_graph_batch(
        &self,
        batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt>;

    fn search_code_symbols(
        &self,
        request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>>;

    fn search_code_references(
        &self,
        request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>>;

    fn search_code_chunks(
        &self,
        request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>>;
}

/// Bounded code symbol search against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSymbolSearchRequest {
    pub source_scope: Option<String>,
    pub path: Option<String>,
    pub name: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}

/// Bounded code reference search against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeReferenceSearchRequest {
    pub source_scope: Option<String>,
    pub path: Option<String>,
    pub symbol_text: Option<String>,
    pub target_symbol_id: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}

/// Bounded code chunk search against an explicit graph snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeChunkSearchRequest {
    pub source_scope: Option<String>,
    pub path: Option<String>,
    pub query: Option<String>,
    pub graph_version: GraphVersion,
    pub limit: usize,
}
