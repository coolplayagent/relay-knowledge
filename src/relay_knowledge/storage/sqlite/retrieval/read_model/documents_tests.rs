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

#[test]
fn bm25_hierarchy_suite_document_lifecycle_keeps_global_and_route_indexes_in_sync() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    super::super::initialize_schema(&connection).expect("schema should initialize");

    insert_code_symbol_document(
        &connection,
        "repo",
        "src/lib.rs",
        "symbol",
        "SearchIndex",
        "struct",
        RetrievalWriteContext {
            graph_version: 1,
            bm25_target: Bm25WriteTarget::Live,
            refresh_labels: true,
            refresh_semantic: true,
            refresh_vector: true,
        },
    )
    .expect("symbol document should insert");
    insert_code_symbol_document(
        &connection,
        "repo",
        "src/lib.rs",
        "symbol",
        "SearchIndex",
        "struct",
        RetrievalWriteContext {
            graph_version: 1,
            bm25_target: Bm25WriteTarget::Live,
            refresh_labels: true,
            refresh_semantic: true,
            refresh_vector: true,
        },
    )
    .expect("replayed symbol document should replace");
    let counts: (usize, usize, usize) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM graph_bm25),
                (SELECT COUNT(*) FROM graph_bm25_route_documents),
                (SELECT document_count FROM graph_bm25_route_state WHERE id=1)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("index counts should load");
    assert_eq!(counts, (1, 1, 1));
    let identity_matches: bool = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1
                 FROM graph_bm25_route_documents route
                 JOIN graph_bm25
                   ON graph_bm25.rowid = route.fts_rowid
                  AND graph_bm25.document_id = route.document_id
             )",
            [],
            |row| row.get(0),
        )
        .expect("FTS route identity should load");
    assert!(identity_matches);
    let (routing_key, group_token): (String, String) = connection
        .query_row(
            "SELECT graph_bm25.routing_key, route.group_token
             FROM graph_bm25_route_documents route
             JOIN graph_bm25 ON graph_bm25.rowid = route.fts_rowid",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("persisted routing tokens should load");
    let expected_scope_token = super::super::super::bm25_routing::scope_token("repo");
    assert_eq!(routing_key, format!("{expected_scope_token} {group_token}"));
    assert!(group_token.starts_with("rkg"));
    let delete_plan = connection
        .prepare(
            "EXPLAIN QUERY PLAN
             DELETE FROM graph_bm25 WHERE rowid = ?1 AND document_id = ?2",
        )
        .expect("rowid delete plan should prepare")
        .query_map(rusqlite::params![1_i64, "missing"], |row| {
            row.get::<_, String>(3)
        })
        .expect("rowid delete plan should run")
        .collect::<Result<Vec<_>, _>>()
        .expect("rowid delete plan should load")
        .join("\n");
    assert!(
        delete_plan.contains("VIRTUAL TABLE INDEX 0:="),
        "{delete_plan}"
    );

    delete_code_documents(&connection, "repo", "src/lib.rs", 2)
        .expect("symbol document should delete");
    let remaining: (usize, usize) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM graph_bm25),
                (SELECT COUNT(*) FROM graph_bm25_route_documents)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("route count should load");
    assert_eq!(remaining, (0, 0));
}

#[test]
fn bm25_hierarchy_suite_code_document_identity_is_unambiguous_across_scopes() {
    assert_ne!(
        code_document_id("symbol", "a", "b:c", "d"),
        code_document_id("symbol", "a:b", "c", "d")
    );
    assert_ne!(
        code_document_id("chunk", "scope", "a:b", "c"),
        code_document_id("chunk", "scope", "a", "b:c")
    );
}

#[test]
fn bm25_hierarchy_suite_path_delete_preserves_cross_layer_batch_identity() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    super::super::initialize_schema(&connection).expect("schema should initialize");
    let write = RetrievalWriteContext {
        graph_version: 1,
        bm25_target: Bm25WriteTarget::Live,
        refresh_labels: true,
        refresh_semantic: true,
        refresh_vector: true,
    };
    for index in 0..257 {
        insert_code_symbol_document(
            &connection,
            "scope",
            "src/large.rs",
            &format!("symbol-{index:03}"),
            &format!("Symbol{index:03}"),
            "struct",
            write,
        )
        .expect("batched symbol should insert");
    }
    insert_code_symbol_document(
        &connection,
        "scope",
        "src/retained.rs",
        "retained-path",
        "RetainedPath",
        "struct",
        write,
    )
    .expect("other path should insert");
    insert_code_symbol_document(
        &connection,
        "other-scope",
        "src/large.rs",
        "retained-scope",
        "RetainedScope",
        "struct",
        write,
    )
    .expect("other scope should insert");

    delete_code_documents(&connection, "scope", "src/large.rs", 2)
        .expect("two path batches should delete");

    let counts: (usize, usize, usize, usize, usize, usize) = connection
        .query_row(
            "SELECT
                 (SELECT COUNT(*) FROM graph_bm25),
                 (SELECT COUNT(*) FROM graph_bm25_route_documents),
                 (SELECT COUNT(*) FROM graph_semantic_documents),
                 (SELECT COUNT(*) FROM graph_vector_documents),
                 (SELECT document_count FROM graph_bm25_route_state WHERE id = 1),
                 (SELECT COUNT(*)
                  FROM graph_bm25_label_grams grams
                  LEFT JOIN graph_bm25_route_documents route
                    ON route.document_id = grams.document_id
                  WHERE route.document_id IS NULL)",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("cross-layer counts should load");
    assert_eq!(counts, (2, 2, 2, 2, 2, 0));
    let retained_identity_count: usize = connection
        .query_row(
            "SELECT COUNT(*)
             FROM graph_bm25_route_documents route
             JOIN graph_bm25
               ON graph_bm25.rowid = route.fts_rowid
              AND graph_bm25.document_id = route.document_id",
            [],
            |row| row.get(0),
        )
        .expect("retained identities should load");
    assert_eq!(retained_identity_count, 2);
}

#[test]
fn bm25_hierarchy_suite_persists_observable_fuzzy_label_degradation() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
        .execute_batch("CREATE TABLE evidence (status TEXT NOT NULL);")
        .expect("evidence table should exist");
    super::super::initialize_schema(&connection).expect("schema should initialize");
    let oversized_label = "x".repeat(super::super::super::label_trigrams::MAX_LABEL_UTF8_BYTES + 1);

    insert_code_symbol_document(
        &connection,
        "scope",
        "src/lib.rs",
        "oversized-label",
        &oversized_label,
        "struct",
        RetrievalWriteContext {
            graph_version: 1,
            bm25_target: Bm25WriteTarget::Live,
            refresh_labels: true,
            refresh_semantic: true,
            refresh_vector: true,
        },
    )
    .expect("authoritative derived document should remain writable");

    let (state, route_graph_version, bm25_count, semantic_count, vector_count, gram_count): (
        String,
        u64,
        usize,
        usize,
        usize,
        usize,
    ) = connection
        .query_row(
            "SELECT
                 (SELECT label_gram_state FROM graph_bm25_route_documents),
                 (SELECT created_graph_version FROM graph_bm25_route_documents),
                 (SELECT COUNT(*) FROM graph_bm25),
                 (SELECT COUNT(*) FROM graph_semantic_documents),
                 (SELECT COUNT(*) FROM graph_vector_documents),
                 (SELECT COUNT(*) FROM graph_bm25_label_grams)",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("label degradation state should load");
    assert_eq!(state, "skipped:label_utf8_bytes");
    assert_eq!(route_graph_version, 1);
    assert_eq!(
        (bm25_count, semantic_count, vector_count, gram_count),
        (1, 1, 1, 0)
    );
}
