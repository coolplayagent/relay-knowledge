use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy, RepositoryCodeRange};

#[test]
fn earlier_sites_receive_a_larger_bounded_bonus() {
    let rows = vec![
        call_row(30, "finish"),
        call_row(10, "prepare"),
        call_row(20, "run"),
    ];
    let request = CodeRetrievalRequest::new(
        "Pipeline.execute",
        CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate"),
        CodeQueryKind::Callees,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let order = callee_execution_order(&rows, &request);

    assert!(
        callee_execution_order_bonus(&order, &rows[1], &request)
            > callee_execution_order_bonus(&order, &rows[0], &request)
    );
    assert_eq!(order.len(), 3);
}

fn call_row(line: u32, callee_name: &str) -> CallRow {
    CallRow {
        file_id: "file".to_owned(),
        path: "src/pipeline.rs".to_owned(),
        language_id: "rust".to_owned(),
        caller_symbol_snapshot_id: Some("caller".to_owned()),
        caller_name: Some("execute".to_owned()),
        callee_symbol_snapshot_id: None,
        callee_name: callee_name.to_owned(),
        line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        caller_line_range: Some(RepositoryCodeRange { start: 1, end: 40 }),
        target_hint: None,
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        caller_canonical_symbol_id: None,
        callee_canonical_symbol_id: None,
        caller_signature: None,
        callee_signature: None,
        caller_excerpt: None,
        callee_excerpt: None,
        is_generated: false,
    }
}
