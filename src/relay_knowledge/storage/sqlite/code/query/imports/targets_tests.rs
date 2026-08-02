use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn usage_context_is_limited_to_symbol_backed_import_queries() {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let plain = CodeRetrievalRequest::new(
        "./protocol",
        selector.clone(),
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    let target_symbol = CodeRetrievalRequest::new(
        "StreamEnvelope",
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    assert!(!import_usage_context_needed(
        &plain,
        &[import_row("./protocol")]
    ));
    assert!(import_usage_context_needed(
        &target_symbol,
        &[import_row("./protocol")]
    ));

    let mut row = import_row("./protocol");
    row.target_symbol_names = Some("StreamEnvelope".to_owned());
    assert!(import_usage_context_needed(&plain, &[row]));
}

fn import_row(module: &str) -> ImportRow {
    ImportRow {
        file_id: "file".to_owned(),
        path: "src/provider.ts".to_owned(),
        language_id: "typescript".to_owned(),
        is_generated: false,
        module: module.to_owned(),
        matched_symbol_name: None,
        target_symbol_names: None,
        same_file_query_usage_count: 0,
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        target_hint: None,
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 0,
        confidence_tier: "none".to_owned(),
    }
}
