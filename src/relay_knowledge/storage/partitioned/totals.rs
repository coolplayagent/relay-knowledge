use std::sync::Arc;

use crate::{
    domain::{CodeParseStatusCounts, CodeRepositoryTotals, CodeSymbolGenerationCounts},
    storage::{CodeRepositoryStore, SqliteGraphStore, StorageError},
};

use super::{catalog::SqliteShardCatalog, routing::source_scope_store};

pub(super) async fn code_repository_totals(
    control: Arc<SqliteGraphStore>,
    catalog: Arc<SqliteShardCatalog>,
) -> Result<CodeRepositoryTotals, StorageError> {
    let repository_ids = catalog.repository_ids().await?;
    let mut totals = control
        .code_repository_totals_excluding(repository_ids.clone())
        .await?;
    for repository_id in repository_ids {
        let Some(shard) = catalog.existing_repository_store(repository_id).await? else {
            continue;
        };
        let shard_totals = shard.code_repository_totals().await?;
        add_code_repository_totals(&mut totals, shard_totals);
    }
    Ok(totals)
}

pub(super) async fn scope_symbol_generation_counts(
    control: Arc<SqliteGraphStore>,
    catalog: Arc<SqliteShardCatalog>,
    source_scope: String,
) -> Result<CodeSymbolGenerationCounts, StorageError> {
    if let Some(shard) = source_scope_store(&catalog, source_scope.clone()).await? {
        return shard
            .code_repository_scope_symbol_generation_counts(source_scope)
            .await;
    }
    control
        .code_repository_scope_symbol_generation_counts(source_scope)
        .await
}

pub(super) fn add_code_repository_totals(
    left: &mut CodeRepositoryTotals,
    right: CodeRepositoryTotals,
) {
    left.repository_count = left.repository_count.saturating_add(right.repository_count);
    left.indexed_file_count = left
        .indexed_file_count
        .saturating_add(right.indexed_file_count);
    left.symbol_count = left.symbol_count.saturating_add(right.symbol_count);
    left.handwritten_symbol_count = left
        .handwritten_symbol_count
        .saturating_add(right.handwritten_symbol_count);
    left.generated_symbol_count = left
        .generated_symbol_count
        .saturating_add(right.generated_symbol_count);
    left.reference_count = left.reference_count.saturating_add(right.reference_count);
    left.chunk_count = left.chunk_count.saturating_add(right.chunk_count);
    left.degraded_file_count = left
        .degraded_file_count
        .saturating_add(right.degraded_file_count);
    left.parse_status_counts =
        add_parse_status_counts(left.parse_status_counts, right.parse_status_counts);
}

fn add_parse_status_counts(
    left: CodeParseStatusCounts,
    right: CodeParseStatusCounts,
) -> CodeParseStatusCounts {
    CodeParseStatusCounts {
        parsed: left.parsed.saturating_add(right.parsed),
        partial: left.partial.saturating_add(right.partial),
        text_only: left.text_only.saturating_add(right.text_only),
        failed: left.failed.saturating_add(right.failed),
    }
}
