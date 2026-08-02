use super::*;
use crate::domain::{CodeRepositorySelector, FreshnessPolicy};

#[test]
fn inline_usage_bonus_recalls_language_scoped_lambda_call_sites() {
    let request = hybrid_request("kotlin lambda request handler timeout default trim");
    let bonus = language_scoped_inline_usage_chunk_bonus(
        2.0,
        &request.query,
        "fun run(values: List<String>) = values.map { value -> client.newCall(value) }",
        "src/main/kotlin/example/Pipeline.kt",
        "kotlin",
        &request,
    );

    assert_eq!(bonus, 4.5);
}

#[test]
fn inline_usage_bonus_ignores_unscoped_and_test_surfaces() {
    let request = hybrid_request("lambda request handler timeout default trim");
    assert_eq!(
        language_scoped_inline_usage_chunk_bonus(
            2.0,
            &request.query,
            "values.map { value -> client.newCall(value) }",
            "src/main/kotlin/example/Pipeline.kt",
            "kotlin",
            &request,
        ),
        0.0
    );

    let test_request = hybrid_request("kotlin lambda request handler timeout default trim");
    assert_eq!(
        language_scoped_inline_usage_chunk_bonus(
            2.0,
            &test_request.query,
            "values.map { value -> client.newCall(value) }",
            "tests/FakeClient.kt",
            "kotlin",
            &test_request,
        ),
        0.0
    );
}

#[test]
fn inline_usage_bonus_requires_hit_language_to_match_query_language() {
    let request = hybrid_request("kotlin lambda request handler timeout default trim");

    assert_eq!(
        language_scoped_inline_usage_chunk_bonus(
            2.0,
            &request.query,
            "values.map(value => client.newCall(value))",
            "src/main/typescript/example/pipeline.ts",
            "typescript",
            &request,
        ),
        0.0
    );
}

fn hybrid_request(query: &str) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Hybrid,
        12,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}
