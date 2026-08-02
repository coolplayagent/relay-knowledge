use super::*;

#[test]
fn normalized_terms_split_mixed_identifiers_and_preserve_whole_tokens() {
    let terms = normalized_terms("GraphRAGContextPack retry_policy RESTClient W3Connector", 2);

    for term in [
        "graphragcontextpack",
        "graph",
        "rag",
        "context",
        "pack",
        "grcp",
        "retry_policy",
        "retry",
        "policy",
        "rest",
        "client",
        "w3connector",
        "connector",
        "w3c",
    ] {
        assert!(terms.contains(term), "missing term {term}");
    }
    assert!(!terms.contains("w"));
    assert!(!terms.contains("3"));
}

#[test]
fn normalized_terms_can_keep_single_character_terms_for_rerank() {
    let terms = normalized_terms("C API W3", 1);

    assert!(terms.contains("c"));
    assert!(terms.contains("api"));
    assert!(terms.contains("w"));
    assert!(terms.contains("3"));
}

#[test]
fn extend_normalized_terms_matches_owned_collection() {
    let mut extended = BTreeSet::from(["existing".to_owned()]);
    extend_normalized_terms("GraphRAGContextPack retry_policy", 2, &mut extended);

    let mut expected = normalized_terms("GraphRAGContextPack retry_policy", 2);
    expected.insert("existing".to_owned());

    assert_eq!(extended, expected);
}
