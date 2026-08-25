use super::{
    incremental_recovery_progress, incremental_recovery_receipt, publication_recovery_progress,
};
use crate::{
    application::code_repository::indexing::task::CodeIndexTaskLeaseContext,
    domain::{
        CodeIncrementalSummaryReceipt, CodeIndexCheckpoint, CodeIndexPublicationFence,
        CodeIndexResourceBudget,
    },
};

#[test]
fn publication_recovery_reports_the_durable_task_budget() {
    let budget = CodeIndexResourceBudget::new(7, 4096, 123)
        .expect("non-default durable budget should validate");

    let progress = publication_recovery_progress(11, budget);

    assert_eq!(progress.resource_budget, budget);
    assert_eq!(progress.skipped_file_count, 11);
    assert_eq!(progress.checkpoint_file_count, 11);
    assert_eq!(progress.sqlite_write_count, 0);
}

#[test]
fn incremental_publication_recovery_preserves_receipt_metrics_and_identity() {
    let budget = CodeIndexResourceBudget::new(7, 4096, 123)
        .expect("non-default durable budget should validate");
    let receipt = CodeIncrementalSummaryReceipt {
        task_id: "task".to_owned(),
        base_resolved_commit_sha: "base".to_owned(),
        changed_path_count: 2,
        skipped_unchanged_count: 9,
        deleted_path_count: 1,
        affected_path_count: 2,
        blob_read_count: 1,
        parsed_file_count: 1,
        sqlite_write_count: 8,
        degraded_file_count: 0,
        batch_count: 1,
    };
    let checkpoint = checkpoint(budget, receipt.clone());
    let lease = lease(budget);

    assert_eq!(
        incremental_recovery_receipt(Some(&checkpoint), &lease)
            .expect("receipt should match its live task checkpoint"),
        Some(receipt.clone())
    );
    let progress = incremental_recovery_progress(&receipt, budget);

    assert_eq!(progress.git_file_count, 2);
    assert_eq!(progress.blob_read_count, 1);
    assert_eq!(progress.parsed_file_count, 1);
    assert_eq!(progress.sqlite_write_count, 8);
    assert_eq!(progress.skipped_file_count, 9);
    assert_eq!(progress.batch_count, 1);
}

#[test]
fn incremental_publication_recovery_ignores_a_previous_tasks_receipt_on_adoption() {
    let budget = CodeIndexResourceBudget::new(7, 4096, 123)
        .expect("non-default durable budget should validate");
    let mut receipt = checkpoint(
        budget,
        CodeIncrementalSummaryReceipt {
            task_id: "task".to_owned(),
            base_resolved_commit_sha: "base".to_owned(),
            changed_path_count: 1,
            skipped_unchanged_count: 0,
            deleted_path_count: 0,
            affected_path_count: 1,
            blob_read_count: 1,
            parsed_file_count: 1,
            sqlite_write_count: 1,
            degraded_file_count: 0,
            batch_count: 1,
        },
    )
    .incremental_summary
    .expect("receipt should exist");
    receipt.task_id = "other-task".to_owned();

    let recovered =
        incremental_recovery_receipt(Some(&checkpoint(budget, receipt)), &lease(budget))
            .expect("content adoption should ignore historical task metrics");

    assert_eq!(recovered, None);
}

fn checkpoint(
    budget: CodeIndexResourceBudget,
    receipt: CodeIncrementalSummaryReceipt,
) -> CodeIndexCheckpoint {
    CodeIndexCheckpoint {
        repository_id: "repo".to_owned(),
        source_scope: "target".to_owned(),
        resolved_commit_sha: "head".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        state: "completed".to_owned(),
        total_path_count: 10,
        parsed_file_count: 10,
        committed_file_count: 10,
        committed_symbol_count: 0,
        committed_reference_count: 0,
        committed_chunk_count: 0,
        committed_fact_row_count: 10,
        incremental_summary: Some(receipt),
        batch_count: 1,
        last_path: Some("src/lib.rs".to_owned()),
        resource_budget: budget,
        updated_at_ms: 1,
    }
}

fn lease(budget: CodeIndexResourceBudget) -> CodeIndexTaskLeaseContext {
    CodeIndexTaskLeaseContext {
        task_id: "task".to_owned(),
        lease_owner: "worker".to_owned(),
        attempt_count: 1,
        lease_duration_ms: 1_000,
        publication_fence: CodeIndexPublicationFence {
            repository_id: "repo".to_owned(),
            task_id: "task".to_owned(),
            lease_owner: "worker".to_owned(),
            attempt_count: 1,
            generation: 1,
        },
        source_scope: "target".to_owned(),
        resolved_commit_sha: "head".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        resource_budget: budget,
    }
}
