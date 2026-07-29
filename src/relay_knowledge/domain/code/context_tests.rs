use super::*;

#[test]
fn context_request_rejects_empty_query() {
    let error = request(" ", 1, 1024).expect_err("empty query should fail");

    assert!(error.to_string().contains("query"));
}

#[test]
fn context_request_bounds_limit_and_context_bytes() {
    assert!(request("retry", 0, 1024).is_err());
    assert!(request("retry", CODEGRAPH_CONTEXT_MAX_LIMIT + 1, 1024).is_err());
    assert!(request("retry", 1, 0).is_err());
    assert!(request("retry", 1, CODEGRAPH_CONTEXT_MIN_BYTES - 1).is_err());
    assert!(request("retry", 1, CODEGRAPH_CONTEXT_MAX_BYTES + 1).is_err());
    assert!(
        request(
            "retry",
            CODEGRAPH_CONTEXT_MAX_LIMIT,
            CODEGRAPH_CONTEXT_MIN_BYTES
        )
        .is_ok()
    );
}

#[test]
fn context_request_defaults_optional_code_toggles_from_json() {
    let request: CodeGraphContextRequest = serde_json::from_value(serde_json::json!({
        "repository": {
            "repository": "repo",
            "ref_selector": "HEAD",
            "path_filters": [],
            "language_filters": []
        },
        "query": "retry",
        "limit": 1,
        "freshness_policy": "allow_stale",
        "max_context_bytes": CODEGRAPH_CONTEXT_MIN_BYTES
    }))
    .expect("request should deserialize with default toggles");

    assert!(request.include_code);
    assert!(!request.exclude_generated);
}

fn request(
    query: &str,
    limit: usize,
    max_context_bytes: usize,
) -> Result<CodeGraphContextRequest, DomainError> {
    CodeGraphContextRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())?,
        query,
        limit,
        FreshnessPolicy::AllowStale,
        max_context_bytes,
        true,
        false,
    )
}
