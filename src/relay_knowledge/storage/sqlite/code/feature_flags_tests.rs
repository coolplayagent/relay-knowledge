use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn feature_flag_sql_applies_filters_and_limit_before_usage_lookup() {
    let selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["./src/payments".to_owned()],
        vec!["rust".to_owned()],
    )
    .expect("selector should validate");
    let request = CodeFeatureFlagRequest::new(
        Some("CHECKOUT_V2".to_owned()),
        selector,
        1,
        FreshnessPolicy::AllowStale,
    )
    .expect("feature flag request should validate");
    let terms = request
        .query
        .as_deref()
        .map(query_terms)
        .unwrap_or_default();

    let query = feature_flag_sql_query("scope", &status(), &request, &terms);

    assert!(query.sql.contains("WITH filtered_flags AS"));
    assert!(query.sql.contains("LIMIT ?"));
    assert_eq!(query.sql.matches("flag.source_scope = ?").count(), 2);
    assert_eq!(
        query
            .sql
            .matches("flag.path = ? OR flag.path LIKE ? ESCAPE '\\'")
            .count(),
        4
    );
    assert_eq!(query.sql.matches("flag.language_id IN").count(), 4);
    assert!(query.sql.contains("lower(flag.source_key) LIKE ?"));
    assert_eq!(query.params.len(), 27);
    assert!(query.params.contains(&Value::Integer(1)));
    assert!(
        query
            .params
            .contains(&Value::Text("src/payments/%".to_owned()))
    );
    assert!(
        query
            .params
            .contains(&Value::Text("%checkout\\_v2%".to_owned()))
    );
}

fn status() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: vec!["src".to_owned()],
        language_filters: vec!["rust".to_owned()],
        last_indexed_scope_id: Some("scope".to_owned()),
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree".to_owned()),
        state: "indexed".to_owned(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}
