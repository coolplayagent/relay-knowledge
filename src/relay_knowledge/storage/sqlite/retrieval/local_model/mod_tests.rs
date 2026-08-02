//! Direct deterministic local-model scoring invariants.

use std::collections::BTreeSet;

use super::*;

#[test]
fn token_signature_and_vector_are_deterministic() {
    let labels = vec!["Rust".to_owned()];
    let signature = token_signature("Async Rust graph", &labels, Some("src/lib.rs"));
    let first = hashed_vector("Async Rust graph", &labels, Some("src/lib.rs"), 8);
    let second = hashed_vector("Async Rust graph", &labels, Some("src/lib.rs"), 8);

    assert!(signature.contains(&"rust".to_owned()));
    assert_eq!(first, second);
    assert!((cosine_similarity(&first, &second) - 1.0).abs() < 0.000_001);
    assert!(hashed_vector("ignored", &[], None, 0).is_empty());
}

#[test]
fn token_signature_adds_identifier_parts_for_semantic_and_vector_recall() {
    let labels = vec!["SemanticVectorRecall".to_owned()];
    let signature = token_signature("GraphRAGContextPack", &labels, None);

    for term in [
        "semantic", "vector", "recall", "graph", "rag", "context", "pack",
    ] {
        assert!(signature.contains(&term.to_owned()), "missing term {term}");
    }
}

#[test]
fn similarity_scores_enforce_empty_and_dimension_boundaries() {
    let query_terms = BTreeSet::from(["graph".to_owned()]);
    let document_terms = BTreeSet::from(["graph".to_owned(), "rust".to_owned()]);

    assert_eq!(semantic_overlap_score(&query_terms, &document_terms), 1.5);
    assert_eq!(
        semantic_overlap_score(&BTreeSet::new(), &document_terms),
        0.0
    );
    assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
    assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
    assert_eq!(cosine_similarity(&[1.0], &[-1.0]), 0.0);
}

#[test]
fn overlap_score_matches_identifier_variants_after_fast_path_miss() {
    let labels = vec!["RuntimeBudget".to_owned()];

    assert_eq!(
        overlap_score(
            "retry_policy",
            "Retry policy controls the runtime budget",
            &labels,
            Some("src/runtime/budget.rs"),
        ),
        2.0
    );
}
