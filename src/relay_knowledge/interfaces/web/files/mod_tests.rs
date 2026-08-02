//! Direct tests for Web file-operation request mapping.

use serde_json::json;

use super::*;

#[test]
fn file_requests_preserve_scope_limits_and_freshness() {
    let payload = json!({
        "query": "database",
        "source_scope": "local-files",
        "root_id": "root-1",
        "limit": 7,
        "freshness": "wait-until-fresh"
    });

    let path_request = file_query_request(&payload).expect("path request should map");
    let content_request = file_content_request(&payload).expect("content request should map");

    assert_eq!(path_request.query, "database");
    assert_eq!(path_request.source_scope.as_deref(), Some("local-files"));
    assert_eq!(path_request.root_id.as_deref(), Some("root-1"));
    assert_eq!(path_request.limit, 7);
    assert_eq!(
        path_request.freshness_policy,
        FreshnessPolicy::WaitUntilFresh
    );
    assert_eq!(content_request.query, path_request.query);
    assert_eq!(content_request.source_scope, path_request.source_scope);
    assert_eq!(content_request.root_id, path_request.root_id);
    assert_eq!(content_request.limit, path_request.limit);
    assert_eq!(
        content_request.freshness_policy,
        path_request.freshness_policy
    );

    let default_request = file_query_request(&json!({ "query": "database", "limit": 7 }))
        .expect("default request should map");
    assert_eq!(
        default_request.freshness_policy,
        FreshnessPolicy::AllowStale
    );
}

#[test]
fn file_index_request_keeps_optional_roots_bounded_by_payload_validation() {
    let request = file_index_request(&json!({
        "source_scope": "local-files",
        "roots": ["/srv/docs", "/srv/runbooks"]
    }))
    .expect("index request should map");

    assert_eq!(request.source_scope.as_deref(), Some("local-files"));
    assert_eq!(request.roots, ["/srv/docs", "/srv/runbooks"]);
    assert!(file_index_request(&json!({ "roots": ["/srv/docs", 42] })).is_err());
}
