//! Produces final counters from durable checkpoint and published scope evidence.
use super::*;

pub(super) fn build_summary(
    connection: &mut Connection,
    session: &CodeIndexSession,
) -> Result<CodeIndexSummary, StorageError> {
    let status =
        status::repository_scope_status_by_source_scope(connection, &session.source_scope)?
            .ok_or_else(|| {
                StorageError::InvalidInput(
                    "code repository scope is missing after index".to_owned(),
                )
            })?;
    let checkpoint = checkpoint::load(connection, &session.source_scope)?;
    let sqlite_write_count = checkpoint::count_scope_rows(connection, &session.source_scope)?;
    let symbol_generation_counts =
        report::scope_symbol_generation_counts(connection, &session.source_scope)?;
    let degraded_file_count =
        checkpoint::count_scope_diagnostics(connection, status.last_indexed_scope_id.as_deref())?;
    let incremental = checkpoint.incremental_summary.as_ref();

    Ok(CodeIndexSummary {
        repository_id: session.repository_id.clone(),
        source_scope: session.source_scope.clone(),
        base_resolved_commit_sha: incremental
            .map(|receipt| receipt.base_resolved_commit_sha.clone())
            .or_else(|| session.base_resolved_commit_sha.clone()),
        resolved_commit_sha: session.resolved_commit_sha.clone(),
        tree_hash: session.tree_hash.clone(),
        indexed_file_count: status.indexed_file_count,
        changed_path_count: incremental
            .map(|receipt| receipt.changed_path_count)
            .unwrap_or(session.changed_path_count),
        skipped_unchanged_count: incremental
            .map(|receipt| receipt.skipped_unchanged_count)
            .unwrap_or(session.skipped_unchanged_count),
        deleted_path_count: incremental
            .map(|receipt| receipt.deleted_path_count)
            .unwrap_or(session.deleted_paths.len()),
        symbol_count: status.symbol_count,
        handwritten_symbol_count: symbol_generation_counts.handwritten,
        generated_symbol_count: symbol_generation_counts.generated,
        reference_count: status.reference_count,
        chunk_count: status.chunk_count,
        degraded_file_count: incremental
            .map(|receipt| receipt.degraded_file_count)
            .unwrap_or(degraded_file_count),
        progress: CodeIndexProgressSummary {
            io_skipped_file_count: incremental
                .map(|r| r.io_skipped_file_count)
                .unwrap_or(status.content_integrity.io_skipped_file_count.unwrap_or(0)),
            io_skipped_directory_count: incremental
                .map(|r| r.io_skipped_directory_count)
                .unwrap_or(
                    status
                        .content_integrity
                        .io_skipped_directory_count
                        .unwrap_or(0),
                ),
            git_file_count: incremental
                .map(|receipt| receipt.changed_path_count)
                .unwrap_or(session.total_path_count),
            blob_read_count: incremental
                .map(|receipt| receipt.blob_read_count)
                .unwrap_or(checkpoint.committed_file_count),
            parsed_file_count: incremental
                .map(|receipt| receipt.parsed_file_count)
                .unwrap_or(checkpoint.parsed_file_count),
            sqlite_write_count: incremental
                .map(|receipt| receipt.sqlite_write_count)
                .unwrap_or(sqlite_write_count),
            skipped_file_count: incremental
                .map(|receipt| receipt.skipped_unchanged_count)
                .unwrap_or(session.skipped_unchanged_count),
            degraded_file_count: incremental
                .map(|receipt| receipt.degraded_file_count)
                .unwrap_or(degraded_file_count),
            batch_count: incremental
                .map(|receipt| receipt.batch_count)
                .unwrap_or(checkpoint.batch_count),
            checkpoint_file_count: incremental
                .map(|receipt| receipt.parsed_file_count)
                .unwrap_or(checkpoint.committed_file_count),
            resource_budget: session.resource_budget,
        },
    })
}
