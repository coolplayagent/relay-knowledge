use super::*;
#[test]
fn resolves_one_distinct_identity_and_rejects_missing_ambiguous_or_failed_definitions() {
    let mut result = CommandResult {
        name: "definition".into(),
        command: vec![],
        exit_code: 0,
        duration_ms: 1,
        stdout: String::new(),
        stderr: String::new(),
    };
    for ids in [vec![], vec!["repo://one", "repo://two"]] {
        result.stdout = serde_json::json!({"results":ids.into_iter().map(|id| serde_json::json!({"canonical_symbol_id":id})).collect::<Vec<_>>()} ).to_string();
        assert!(resolve_selector(&result).is_err());
    }
    result.stdout = serde_json::json!({"results":[{"canonical_symbol_id":"repo://one"},{"canonical_symbol_id":"repo://one"}]}).to_string();
    assert_eq!(resolve_selector(&result).unwrap(), "repo://one");
    result.exit_code = 1;
    assert!(resolve_selector(&result).is_err());
}
