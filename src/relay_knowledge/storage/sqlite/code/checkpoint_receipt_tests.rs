use super::{decode, encode};
use crate::domain::{CodeIncrementalSummaryReceipt, CodeIndexResourceBudget};

fn deletion_only_receipt() -> CodeIncrementalSummaryReceipt {
    CodeIncrementalSummaryReceipt {
        task_id: "task-delete".to_owned(),
        base_resolved_commit_sha: "base".to_owned(),
        changed_path_count: 1,
        skipped_unchanged_count: 9,
        deleted_path_count: 1,
        affected_path_count: 1,
        blob_read_count: 0,
        parsed_file_count: 0,
        sqlite_write_count: 0,
        degraded_file_count: 0,
        batch_count: 1,
    }
}

#[test]
fn deletion_only_receipt_preserves_the_direct_single_batch_contract() {
    let receipt = deletion_only_receipt();
    let encoded = encode(&receipt).expect("deletion-only receipt should encode");
    let budget = CodeIndexResourceBudget::new(1, 4096, 8).expect("budget should validate");

    let decoded = decode(Some(encoded), 0, budget)
        .expect("canonical receipt should decode")
        .expect("receipt should be present");

    assert_eq!(decoded, receipt);
    assert_eq!(decoded.batch_count, 1);
    assert_eq!(decoded.blob_read_count, 0);
    assert_eq!(decoded.deleted_path_count, 1);
}

#[test]
fn receipt_decode_rejects_metrics_outside_the_frozen_budget() {
    let mut receipt = deletion_only_receipt();
    receipt.sqlite_write_count = 9;
    let encoded = encode(&receipt).expect("shape-valid receipt should encode");
    let budget = CodeIndexResourceBudget::new(1, 4096, 8).expect("budget should validate");

    let error = decode(Some(encoded), 0, budget)
        .expect_err("checkpoint decode must enforce its frozen row budget");

    assert!(error.to_string().contains("frozen resource budget"));
}

#[test]
fn receipt_decode_rejects_noncanonical_or_unbound_identity() {
    let budget = CodeIndexResourceBudget::new(1, 4096, 8).expect("budget should validate");
    let noncanonical = format!(" {}", encode(&deletion_only_receipt()).expect("receipt"));
    assert!(decode(Some(noncanonical), 0, budget).is_err());

    let mut missing_task = deletion_only_receipt();
    missing_task.task_id.clear();
    assert!(encode(&missing_task).is_err());
}

#[test]
fn filtered_diff_metrics_are_not_bound_to_the_delta_file_quantum() {
    let mut receipt = deletion_only_receipt();
    receipt.changed_path_count = 100;
    receipt.deleted_path_count = 0;
    receipt.affected_path_count = 0;
    let encoded = encode(&receipt).expect("filtered diff receipt should encode");
    let budget = CodeIndexResourceBudget::new(1, 4096, 8).expect("budget should validate");

    let decoded = decode(Some(encoded), 0, budget)
        .expect("raw changed-path metrics must not consume the snapshot file quantum")
        .expect("receipt should be present");

    assert_eq!(decoded.changed_path_count, 100);
    assert_eq!(decoded.affected_path_count, 0);
}

#[test]
fn receipt_decode_enforces_the_exact_frozen_byte_boundary() {
    let encoded = encode(&deletion_only_receipt()).expect("receipt should encode");
    let exact_budget = CodeIndexResourceBudget::new(1, encoded.len(), 8)
        .expect("exact byte budget should validate");
    assert!(decode(Some(encoded.clone()), 0, exact_budget).is_ok());

    let short_budget = CodeIndexResourceBudget::new(1, encoded.len() - 1, 8)
        .expect("one-byte-short budget should validate");
    let error = decode(Some(encoded), 0, short_budget)
        .expect_err("receipt must not exceed its frozen checkpoint byte quantum");
    assert!(error.to_string().contains("frozen resource budget"));
}
