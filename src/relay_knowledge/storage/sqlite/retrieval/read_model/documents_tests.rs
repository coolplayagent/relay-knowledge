use rusqlite::Connection;

use super::*;
use crate::domain::EvidenceModality;

#[test]
fn semantic_document_stores_source_hash_without_retrieval_token_noise() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist for retrieval migration checks");
    super::super::initialize_schema(&connection).expect("schema should initialize");
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
fn label_decoding_accepts_json_and_legacy_separator_rows() {
    assert_eq!(
        split_labels("[\"alpha\",\"beta\"]".to_owned()),
        vec!["alpha".to_owned(), "beta".to_owned()]
    );
    assert_eq!(
        split_labels("alpha\u{1f}beta".to_owned()),
        vec!["alpha".to_owned(), "beta".to_owned()]
    );
}
