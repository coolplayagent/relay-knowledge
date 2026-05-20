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
fn semantic_document_stores_source_hash_without_retrieval_token_noise() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist for retrieval migration checks");
    initialize_schema(&connection).expect("schema should initialize");
    let labels = vec!["SemanticVectorRecall".to_owned()];
    replace_semantic_document(
        &connection,
        SemanticDocumentInput {
            document_id: "doc",
            document_kind: "evidence",
            evidence_id: "ev",
            parent_evidence_id: None,
            modality: EvidenceModality::TextSpan,
            source_scope: "scope",
            source_path: Some("docs/source.md"),
            entity_labels: &labels,
            content: "backend freshness source attribution",
            source_hash: "sha256:abcdef123456",
            graph_version: 1,
            model: LOCAL_SEMANTIC_MODEL,
            dimension: LOCAL_VECTOR_DIMENSION,
        },
    )
    .expect("semantic document should insert");
    let (signature_json, source_hash): (String, String) = connection
        .query_row(
            "
            SELECT token_signature_json, source_hash
            FROM graph_semantic_documents
            WHERE document_id = 'doc'
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("semantic row should load");
    let signature = parse_string_array(&signature_json).expect("signature should parse");

    assert_eq!(source_hash, "sha256:abcdef123456");
    assert!(signature.contains(&"backend".to_owned()));
    assert!(signature.contains(&"semantic".to_owned()));
    assert!(signature.contains(&"source".to_owned()));
    assert!(!signature.contains(&"sha256".to_owned()));
    assert!(!signature.contains(&"abcdef123456".to_owned()));
}

#[test]
fn initialize_schema_creates_derived_scope_version_indexes() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist for retrieval migration checks");

    initialize_schema(&connection).expect("schema should initialize");

    let index_count = connection
        .query_row(
            "
            SELECT COUNT(*)
            FROM sqlite_master
            WHERE type = 'index'
              AND name IN (
                'graph_semantic_documents_scope_version',
                'graph_vector_documents_scope_version'
              )
            ",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("derived retrieval indexes should be inspectable");

    assert_eq!(index_count, 2);
}
