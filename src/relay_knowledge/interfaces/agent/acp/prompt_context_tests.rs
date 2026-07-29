use super::AcpPromptResult;

#[test]
fn empty_prompt_result_reports_no_entries_or_truncation() {
    let result = AcpPromptResult {
        retrieval: None,
        codegraph: None,
    };

    assert_eq!(result.result_count(), 0);
    assert!(!result.truncated());
}
