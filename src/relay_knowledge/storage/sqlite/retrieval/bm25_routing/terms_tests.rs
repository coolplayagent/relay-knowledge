use super::*;

#[test]
fn bm25_hierarchy_suite_bounds_query_terms_at_ascii_fts_boundaries() {
    assert_eq!(
        query_terms("HTTP_server retry-policy").expect("query should route"),
        vec!["http", "policy", "retry", "server"]
    );
    assert!(query_terms("向量检索").is_none());
    let oversized = (0..33)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(query_terms(&oversized).is_none());
    assert!(query_terms(&vec!["repeat"; 33].join(" ")).is_none());
}

#[test]
fn inventories_bound_per_document_memory_and_simhash_is_deterministic() {
    let content = (0..300)
        .map(|index| format!("token{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let input = Bm25RoutingText {
        source_scope: "scope",
        source_path: None,
        entity_labels: "",
        entity_aliases: "",
        content: &content,
        graph_version: 1,
    };
    let first = topical_inventory(&input);
    let second = topical_inventory(&input);

    assert_eq!(first.counts.len(), MAX_ROUTING_TERMS_PER_DOCUMENT);
    assert_eq!(simhash_prefix(&first, 10), simhash_prefix(&second, 10));
    assert!(simhash_prefix(&first, 10) < 1_024);
}

#[test]
fn bm25_hierarchy_suite_rejects_oversized_terms_before_allocation() {
    let oversized = "a".repeat(MAX_ROUTING_TERM_BYTES + 1);
    let input = Bm25RoutingText {
        source_scope: "scope",
        source_path: None,
        entity_labels: "",
        entity_aliases: "",
        content: &oversized,
        graph_version: 1,
    };

    assert!(topical_inventory(&input).counts.is_empty());
    assert!(query_terms(&oversized).is_none());
    assert!(query_terms(&format!("common {oversized}")).is_none());
    assert!(query_terms(&vec![oversized.as_str(); 33].join(" ")).is_none());
}

#[test]
fn bm25_hierarchy_suite_routes_only_ascii_terms_that_are_safe_fts_tokens() {
    let input = Bm25RoutingText {
        source_scope: "scope",
        source_path: None,
        entity_labels: "",
        entity_aliases: "",
        content: "plain éfoo fóo foo。bar safe-term",
        graph_version: 1,
    };

    assert_eq!(
        indexed_inventory(&input).counts,
        vec![
            ("plain".to_owned(), 1),
            ("safe".to_owned(), 1),
            ("scope".to_owned(), 1),
            ("term".to_owned(), 1),
        ]
    );
}
