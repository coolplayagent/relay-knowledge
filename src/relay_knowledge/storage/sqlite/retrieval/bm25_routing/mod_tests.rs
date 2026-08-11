use super::{Bm25RoutingText, prepare_document, scope_token};

#[test]
fn bm25_hierarchy_suite_keeps_scope_as_a_hard_route_partition() {
    let first = prepare_document(Bm25RoutingText {
        source_scope: "repository-a",
        source_path: Some("src/retrieval.rs"),
        entity_labels: "[\"SearchIndex\"]",
        entity_aliases: "search index",
        content: "lexical ranking inverted index postings",
        graph_version: 7,
    });
    let other_scope = prepare_document(Bm25RoutingText {
        source_scope: "repository-b",
        source_path: Some("src/retrieval.rs"),
        entity_labels: "[\"SearchIndex\"]",
        entity_aliases: "search index",
        content: "lexical ranking inverted index postings",
        graph_version: 7,
    });

    assert_ne!(first.group_token, other_scope.group_token);
    let first_scope_token = scope_token("repository-a");
    let other_scope_token = scope_token("repository-b");
    assert_ne!(first_scope_token, other_scope_token);
    assert!(first_scope_token.starts_with("rks"));
    assert!(first.group_token.starts_with("rkg"));
    assert_eq!(
        first.routing_key,
        format!("{first_scope_token} {}", first.group_token)
    );
    assert_eq!(
        other_scope.routing_key,
        format!("{other_scope_token} {}", other_scope.group_token)
    );
    assert_ne!(first_scope_token, first.group_token);
}
