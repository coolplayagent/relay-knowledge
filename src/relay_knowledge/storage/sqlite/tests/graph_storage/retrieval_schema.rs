//! Retrieval schema scenarios exercised through the SQLite graph facade.

use super::*;

#[tokio::test]
async fn startup_rebuilds_obsolete_bm25_schema_without_deleting_graph_data() {
    let path = temp_db_path("bm25-reset");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        store
            .commit_code_graph_batch(
                CodeGraphBatch::new(vec![parsed_code_file("repo", "src/lib.rs", "sym-main")])
                    .expect("batch should validate"),
            )
            .await
            .expect("code graph commit should succeed");
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute("DROP TABLE graph_bm25", [])
            .expect("current bm25 table should drop");
        guard
            .execute_batch(
                "
                CREATE VIRTUAL TABLE graph_bm25 USING fts5(
                    document_id UNINDEXED,
                    document_kind UNINDEXED,
                    evidence_id UNINDEXED,
                    source_scope,
                    source_path,
                    entity_labels,
                    content
                );
                ",
            )
            .expect("obsolete bm25 table should be simulated");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    let hits = store
        .search(GraphSearchRequest {
            query: "main".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(graph.graph_version, GraphVersion::new(1));
    assert_eq!(graph.code_file_count, 1);
    assert!(!hits.is_empty());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn startup_recovers_missing_hierarchical_bm25_route_state() {
    let path = temp_db_path("bm25-route-rebuild");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        commit_evidence(
            &store,
            "ev-route-rebuild",
            "docs",
            "Hierarchical lexical routing preserves global ranking",
        )
        .await;
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute("DROP TABLE graph_bm25_route_documents", [])
            .expect("route document state should drop");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let counts: (usize, usize, usize) = {
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM graph_bm25),
                    (SELECT COUNT(*) FROM graph_bm25_route_documents),
                    (SELECT COUNT(*) FROM graph_bm25_route_groups)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("derived index counts should load")
    };
    assert_eq!(counts, (1, 1, 1));

    let graph = store.inspect_graph().await.expect("graph should inspect");
    assert_eq!(graph.evidence_count, 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn startup_rebuilds_missing_hierarchical_bm25_term_state() {
    for table in ["graph_bm25_route_terms", "graph_bm25_route_term_totals"] {
        let path = temp_db_path("bm25-route-terms-rebuild");
        {
            let store = SqliteGraphStore::open(&path).expect("store should open");
            commit_evidence(
                &store,
                "ev-route-term-rebuild",
                "docs",
                "Hierarchical lexical routing maintains aggregate term statistics",
            )
            .await;
            let guard = store.connection.lock().expect("connection should lock");
            guard
                .execute(&format!("DROP TABLE {table}"), [])
                .expect("route term state should drop");
        }

        let store = SqliteGraphStore::open(&path).expect("store should reopen");
        let counts: (usize, usize, String) = {
            let guard = store.connection.lock().expect("connection should lock");
            guard
                .query_row(
                    "SELECT
                        (SELECT COUNT(*) FROM graph_bm25_route_terms),
                        (SELECT COUNT(*) FROM graph_bm25_route_term_totals),
                        (SELECT state FROM graph_bm25_route_state WHERE id = 1)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .expect("rebuilt term state should load")
        };
        assert!(
            counts.0 > 0,
            "group term rows should rebuild after dropping {table}"
        );
        assert!(
            counts.1 > 0,
            "global term rows should rebuild after dropping {table}"
        );
        assert_eq!(counts.2, "fresh");
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn startup_rebuilds_routing_after_pre_v4_writer_marker() {
    let path = temp_db_path("bm25-pre-v4-writer");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        commit_evidence(
            &store,
            "ev-pre-v4-writer",
            "docs",
            "Schema rollback must rebuild hierarchical routing state",
        )
        .await;
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute("UPDATE graph_bm25 SET routing_key = NULL", [])
            .expect("older writer output should be simulated");
        guard
            .execute(
                "UPDATE relay_storage_schema_state
                 SET version = 3
                 WHERE key = 'sqlite_graph_store'",
                [],
            )
            .expect("older schema marker should be simulated");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let state: (usize, String, u64) = {
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM graph_bm25
                     WHERE routing_key IS NOT NULL AND routing_key <> ''),
                    (SELECT state FROM graph_bm25_route_state WHERE id = 1),
                    (SELECT indexed_graph_version FROM graph_bm25_route_state WHERE id = 1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("rebuilt route state should load")
    };
    assert_eq!(state, (1, "fresh".to_owned(), 1));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn relation_only_mutation_advances_bm25_route_generation() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-route-version", "docs", "BM25 route evidence").await;
    let relation = GraphRelationRecord::new(
        "rel-route-version",
        SourceScope::parse("docs").expect("scope should parse"),
        "BM25 route",
        "preserves",
        "global score",
        vec!["ev-route-version".to_owned()],
    )
    .expect("relation should validate");
    let batch = GraphMutationBatch::with_facts(Vec::new(), vec![relation], Vec::new(), Vec::new())
        .expect("relation-only batch should validate");

    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("relation-only mutation should commit");
    assert_eq!(receipt.graph_version, GraphVersion::new(2));
    let state: (u64, usize) = store
        .connection
        .lock()
        .expect("connection should lock")
        .query_row(
            "SELECT indexed_graph_version, document_count
             FROM graph_bm25_route_state WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("route state should load");
    assert_eq!(state, (2, 1));
}

#[tokio::test]
async fn startup_creates_missing_label_gram_table_for_current_schema_marker() {
    let path = temp_db_path("label-grams-current-marker");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        store
            .commit_code_graph_batch(
                CodeGraphBatch::new(vec![parsed_code_file("repo", "src/lib.rs", "sym-main")])
                    .expect("batch should validate"),
            )
            .await
            .expect("code graph commit should succeed");
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute("DROP TABLE graph_bm25_label_grams", [])
            .expect("label gram table should drop");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let hits = store
        .search(GraphSearchRequest {
            query: "maim".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("fuzzy search should use recreated label grams");

    assert!(hits.iter().any(|hit| hit.evidence_id == "sym-main"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn startup_resumes_label_gram_backfill_from_previous_schema_marker() {
    let path = temp_db_path("label-grams-previous-marker");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        store
            .commit_code_graph_batch(
                CodeGraphBatch::new(vec![parsed_code_file("repo", "src/lib.rs", "sym-resume")])
                    .expect("batch should validate"),
            )
            .await
            .expect("code graph commit should succeed");
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute("DELETE FROM graph_bm25_label_grams", [])
            .expect("partial label gram backfill should be simulated");
        guard
            .execute(
                "UPDATE graph_bm25_route_documents
                 SET label_gram_state = 'pending'",
                [],
            )
            .expect("pending label state should be simulated");
        guard
            .execute(
                "
                UPDATE relay_storage_schema_state
                SET version = 1
                WHERE key = 'sqlite_graph_store'
                ",
                [],
            )
            .expect("previous marker should be simulated");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let hits = store
        .search(GraphSearchRequest {
            query: "sym-resune".to_owned(),
            source_scope: Some("repo".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("fuzzy search should use resumed label grams");

    assert!(hits.iter().any(|hit| hit.evidence_id == "sym-resume"));
    let label_state = store
        .connection
        .lock()
        .expect("connection should lock")
        .query_row(
            "SELECT label_gram_state
             FROM graph_bm25_route_documents",
            [],
            |row| row.get::<_, String>(0),
        )
        .expect("backfilled label state should load");
    assert_eq!(label_state, "indexed");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn initialization_backfills_empty_semantic_and_vector_documents() {
    let path = temp_db_path("derived-backfill");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        commit_evidence(
            &store,
            "ev-retry-backfill",
            "docs",
            "Retry policy controls runtime budget",
        )
        .await;
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute("DELETE FROM graph_semantic_documents", [])
            .expect("semantic rows should delete");
        guard
            .execute("DELETE FROM graph_vector_documents", [])
            .expect("vector rows should delete");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let hits = store
        .search(GraphSearchRequest {
            query: "retry_policy".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(hits.iter().any(|hit| {
        hit.retriever_sources.contains(&RetrieverSource::Semantic)
            || hit.retriever_sources.contains(&RetrieverSource::Vector)
    }));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn initialization_rebuilds_derived_documents_when_tokenizer_version_changes() {
    let path = temp_db_path("derived-tokenizer-version");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        let evidence = EvidenceRecord::new(
            "ev-tokenizer-rebuild",
            SourceScope::parse("docs").expect("scope should parse"),
            "Opaque retrieval backend note",
            vec!["GraphRAGContextPack".to_owned()],
        )
        .expect("evidence should validate");
        store
            .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
            .await
            .expect("commit should succeed");
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute(
                "UPDATE graph_semantic_documents
                 SET token_signature_json = '[\"graphragcontextpack\"]',
                     tokenizer_version = 'legacy-tokenizer'",
                [],
            )
            .expect("semantic tokenizer version should downgrade");
        guard
            .execute(
                "UPDATE graph_vector_documents
                 SET vector_json = '[0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0,0.0]',
                     tokenizer_version = 'legacy-tokenizer'",
                [],
            )
            .expect("vector tokenizer version should downgrade");
        guard
            .execute(
                "UPDATE graph_bm25_route_state
                 SET semantic_generation = 'legacy-tokenizer',
                     vector_generation = 'legacy-tokenizer'
                 WHERE id = 1",
                [],
            )
            .expect("persisted companion generations should downgrade");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let hits = store
        .search(GraphSearchRequest {
            query: "context pack".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: vec![
                RetrieverSource::Bm25,
                RetrieverSource::GraphEvidence,
                RetrieverSource::CodeGraph,
                RetrieverSource::GraphPath,
                RetrieverSource::Temporal,
                RetrieverSource::CommunitySummary,
            ],
        })
        .await
        .expect("search should succeed");

    let hit = hits
        .first()
        .expect("rebuilt semantic and vector indexes should return evidence");
    assert_eq!(hit.evidence_id, "ev-tokenizer-rebuild");
    assert!(hit.retriever_sources.contains(&RetrieverSource::Semantic));
    assert!(hit.retriever_sources.contains(&RetrieverSource::Vector));
    assert!(hit.retriever_sources.iter().all(|source| {
        ![
            RetrieverSource::Bm25,
            RetrieverSource::GraphEvidence,
            RetrieverSource::CodeGraph,
            RetrieverSource::GraphPath,
            RetrieverSource::Temporal,
            RetrieverSource::CommunitySummary,
        ]
        .contains(source)
    }));
    let guard = store.connection.lock().expect("connection should lock");
    let current_semantic_rows: usize = guard
        .query_row(
            "SELECT COUNT(*) FROM graph_semantic_documents WHERE tokenizer_version = ?1",
            [super::retrieval::LOCAL_TOKENIZER_VERSION],
            |row| row.get(0),
        )
        .expect("semantic version count should load");
    let current_vector_rows: usize = guard
        .query_row(
            "SELECT COUNT(*) FROM graph_vector_documents WHERE tokenizer_version = ?1",
            [super::retrieval::LOCAL_TOKENIZER_VERSION],
            |row| row.get(0),
        )
        .expect("vector version count should load");
    assert_eq!(current_semantic_rows, 1);
    assert_eq!(current_vector_rows, 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn initialization_rebuilds_partially_populated_retrieval_documents() {
    let path = temp_db_path("partial-derived-backfill");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        commit_evidence(
            &store,
            "ev-partial-keep",
            "docs",
            "Partial rebuild keeps one existing row",
        )
        .await;
        commit_evidence(
            &store,
            "ev-partial-missing",
            "docs",
            "SecondPartialNeedle should be rebuilt",
        )
        .await;
        let guard = store.connection.lock().expect("connection should lock");
        for table in [
            "graph_bm25",
            "graph_semantic_documents",
            "graph_vector_documents",
        ] {
            guard
                .execute(
                    &format!("DELETE FROM {table} WHERE evidence_id = ?1"),
                    ["ev-partial-missing"],
                )
                .expect("partial rows should delete");
        }
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let hits = store
        .search(GraphSearchRequest {
            query: "SecondPartialNeedle".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(
        hits.iter()
            .any(|hit| hit.evidence_id == "ev-partial-missing")
    );
    let _ = std::fs::remove_file(path);
}
