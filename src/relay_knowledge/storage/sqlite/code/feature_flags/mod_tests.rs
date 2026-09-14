//! Direct contracts for feature-flag SQL planning and ranking.

use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn feature_flag_sql_applies_scope_and_bounded_candidate_budget() {
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
        .transpose()
        .unwrap()
        .unwrap_or_default();

    let query = feature_flag_sql_query("scope", &status(), &request, &terms);

    assert!(query.sql.contains("WITH filtered_flags AS"));
    assert!(query.sql.contains("LIMIT ?"));
    // Key seeds, symbolic seeds, and returned usages each enforce authorization.
    assert_eq!(query.sql.matches("flag.source_scope = ?").count(), 3);
    assert_eq!(
        query
            .sql
            .matches("flag.path = ? OR instr(flag.path, ?) = 1")
            .count(),
        6
    );
    assert_eq!(query.sql.matches("flag.language_id IN").count(), 6);
    assert!(
        query
            .sql
            .contains("config_casefold(flag.source_key) LIKE ?")
    );
    assert_eq!(query.params.len(), 29);
    assert!(
        query
            .params
            .contains(&Value::Integer(registry::MAX_ROWS as i64 + 1))
    );
    assert!(
        query
            .params
            .contains(&Value::Text("src/payments/".to_owned()))
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

#[test]
fn configuration_query_terms_reject_overflow_without_truncating() {
    assert_eq!(query_terms(&vec!["x"; 64].join(" ")).unwrap().len(), 64);
    assert_eq!(query_terms(&"x".repeat(256)).unwrap()[0].len(), 256);
    for query in [
        vec!["x"; 65].join(" "),
        vec!["x"; 5000].join(" "),
        "x".repeat(257),
        " ".repeat(10001),
        "İ".repeat(100),
    ] {
        let error = query_terms(&query).unwrap_err();
        assert!(matches!(error, StorageError::InvalidInput(_)));
        assert!(error.to_string().contains("budget exceeded"));
    }
}

#[test]
fn supplied_queries_require_a_searchable_term() {
    for query in ["", " ", ".", "... --", "🧪", "💡.🚀"] {
        assert!(
            query_terms(query)
                .unwrap_err()
                .to_string()
                .contains("alphanumeric"),
            "{query}"
        );
    }
    for (query, expected) in [("_", "_"), ("feature.🧪", "feature"), ("账务", "账务")] {
        assert_eq!(query_terms(query).unwrap(), vec![expected]);
    }
}
