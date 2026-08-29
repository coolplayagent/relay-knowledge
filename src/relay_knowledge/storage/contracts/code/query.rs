use crate::domain::{
    CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeImpactRequest, CodeRepositoryReport,
    CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest, CodeSymbolGenerationCounts,
    CodebaseViewRequest, CodebaseViewSnapshot,
};

use super::super::{StorageError, StorageFuture};
use super::CodeImpactChanges;

/// Snapshot-scoped code retrieval, impact, report, and view read capability.
pub trait CodeQueryReadStore: Send + Sync {
    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>>;

    fn search_code_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code feature flag search for repository '{}' is unavailable",
                request.repository.repository
            )))
        })
    }

    fn search_code_feature_flags_scope(
        &self,
        source_scope: String,
        _request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code feature flag search for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn search_code_scope(
        &self,
        source_scope: String,
        _request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code search for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn analyze_code_impact(
        &self,
        request: CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>>;

    fn analyze_code_impact_scope(
        &self,
        source_scope: String,
        _request: CodeImpactRequest,
        _changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code impact analysis for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn codebase_view_snapshot(
        &self,
        source_scope: String,
        _request: CodebaseViewRequest,
        _row_limit: usize,
    ) -> StorageFuture<'_, CodebaseViewSnapshot> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "codebase view snapshot for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn code_repository_totals(&self) -> StorageFuture<'_, CodeRepositoryTotals> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code repository totals are unavailable".to_owned(),
            ))
        })
    }

    fn code_repository_report(
        &self,
        repository: String,
    ) -> StorageFuture<'_, CodeRepositoryReport> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code repository report for '{repository}' is unavailable"
            )))
        })
    }

    fn code_repository_scope_symbol_generation_counts(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, CodeSymbolGenerationCounts> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code symbol generation counts for source scope '{source_scope}' are unavailable"
            )))
        })
    }
}
