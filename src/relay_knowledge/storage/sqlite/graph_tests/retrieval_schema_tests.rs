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

    assert_eq!(hits[0].evidence_id, "ev-tokenizer-rebuild");
    assert!(
        hits[0]
            .retriever_sources
            .contains(&RetrieverSource::Semantic)
    );
    assert!(hits[0].retriever_sources.contains(&RetrieverSource::Vector));
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
