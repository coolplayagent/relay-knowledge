use super::*;

#[test]
fn clone_checkpoint_round_trips_cursor_and_scan_proof() {
    let state = code_incremental_clone_state(
        CodeIncrementalClonePhase::Search,
        11,
        27,
        41_000,
        "0123456789abcdef",
    )
    .expect("valid token");
    let parsed = code_incremental_clone(&state).expect("canonical token");
    assert_eq!(parsed.phase, CodeIncrementalClonePhase::Search);
    assert_eq!(parsed.table_ordinal, 11);
    assert_eq!(parsed.completed_page_ordinal, 27);
    assert_eq!(parsed.scanned_total_rows, 41_000);
    assert_eq!(parsed.cursor_digest, "0123456789abcdef");
}

#[test]
fn clone_checkpoint_rejects_noncanonical_or_unbounded_cursor_tokens() {
    assert!(code_incremental_clone("staging:incremental_clone:v1:0:00:0:0:none").is_none());
    assert!(
        code_incremental_clone("staging:incremental_clone:v1:0:0:0:0:ABCDEF0123456789").is_none()
    );
    assert!(
        code_incremental_clone_state(CodeIncrementalClonePhase::Tables, 0, 0, 0, "not-a-digest",)
            .is_none()
    );
}
