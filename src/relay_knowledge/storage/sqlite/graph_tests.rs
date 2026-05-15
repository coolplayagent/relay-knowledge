use super::*;
use crate::domain::{
    ClaimRecord, CodeChunkRecord, CodeExtractionMetadata, CodeFileFields, CodeFileRecord,
    CodeGraphBatch, CodeParseStatus, CodeRange, CodeSymbolKind, CodeSymbolRecord, ConfidenceScore,
    EventRecord, EvidenceRecord, FactStatus, GraphRelationRecord, RetrieverSource, SourceScope,
};

#[tokio::test]
async fn commits_graph_batch_and_marks_indexes_stale() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-1",
        scope,
        "Rust uses ownership",
        vec!["Rust".to_owned()],
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");

    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
    let inspection = store.inspect_graph().await.expect("inspection should load");
    let statuses = store.index_statuses().await.expect("statuses should load");

    assert_eq!(receipt.graph_version, GraphVersion::new(1));
    assert_eq!(inspection.entity_count, 1);
    assert_eq!(inspection.evidence_count, 1);
    assert!(
        statuses
            .iter()
            .all(|status| status.is_stale_for(GraphVersion::new(1)))
    );
}

#[tokio::test]
async fn searches_evidence_by_query_token() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new("ev-1", scope, "Hybrid retrieval uses BM25", Vec::new())
        .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "BM25".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].evidence_id, "ev-1");
    assert!(hits[0].retriever_sources.contains(&RetrieverSource::Bm25));
}

#[tokio::test]
async fn bm25_matches_generated_entity_aliases_without_returning_aliases_as_labels() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-alias",
        scope,
        "Alias-only evidence body",
        vec!["GraphRAGContextPack".to_owned()],
    )
    .expect("evidence should validate");
    let receipt = store
        .commit_mutation_batch(GraphMutationBatch::new(vec![evidence]).expect("batch"))
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "context pack".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: receipt.graph_version,
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].evidence_id, "ev-alias");
    assert_eq!(hits[0].entity_labels, ["GraphRAGContextPack"]);
    assert!(hits[0].retriever_sources.contains(&RetrieverSource::Bm25));
}

#[tokio::test]
async fn deterministic_semantic_and_vector_retrieval_match_identifier_variants() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-retry",
        "docs",
        "Retry policy controls the runtime budget",
    )
    .await;

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

    assert_eq!(hits[0].evidence_id, "ev-retry");
    assert!(
        hits[0]
            .retriever_sources
            .contains(&RetrieverSource::Semantic)
            || hits[0].retriever_sources.contains(&RetrieverSource::Vector)
    );
}

#[tokio::test]
async fn derived_retrieval_scores_older_documents_before_truncating() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-older-match",
        "docs",
        "AncientNeedle policy controls runtime budget",
    )
    .await;
    for index in 0..45 {
        commit_evidence(
            &store,
            &format!("ev-filler-{index}"),
            "docs",
            &format!("Recent filler document {index}"),
        )
        .await;
    }

    let hits = store
        .search(GraphSearchRequest {
            query: "ancientneedle".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(46),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");
    let hit = hits
        .iter()
        .find(|hit| hit.evidence_id == "ev-older-match")
        .expect("older matching document should be retained");

    assert!(
        hit.retriever_sources.contains(&RetrieverSource::Semantic)
            || hit.retriever_sources.contains(&RetrieverSource::Vector)
    );
}

#[tokio::test]
async fn code_read_model_hits_merge_with_bm25_document() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    store
        .commit_code_graph_batch(
            CodeGraphBatch::new(vec![parsed_code_file("repo", "src/lib.rs", "sym-main")])
                .expect("batch should validate"),
        )
        .await
        .expect("code graph commit should succeed");

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
    let symbol_hit = hits
        .iter()
        .find(|hit| {
            hit.code_artifact
                .as_ref()
                .is_some_and(|artifact| artifact.artifact_id == "sym-main")
        })
        .expect("symbol hit should be present");

    assert!(
        symbol_hit
            .retriever_sources
            .contains(&RetrieverSource::CodeGraph)
    );
    assert!(
        symbol_hit
            .retriever_sources
            .contains(&RetrieverSource::Semantic)
    );
    assert!(
        symbol_hit
            .retriever_sources
            .contains(&RetrieverSource::Vector)
    );
}

#[tokio::test]
async fn reads_mutation_log_after_version() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Rust async storage").await;
    commit_evidence(&store, "ev-2", "docs", "SQLite graph storage").await;

    let entries = store
        .read_after(GraphVersion::new(1), 10)
        .await
        .expect("mutation log should load");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].graph_version, GraphVersion::new(2));
    assert_eq!(entries[0].evidence_count, 1);
    assert_eq!(entries[0].affected_scopes, ["docs"]);
    assert_eq!(entries[0].evidence_ids, ["ev-2"]);
    assert_eq!(entries[0].source_hashes.len(), 1);
    assert_eq!(entries[0].affected_entity_ids.len(), 1);
}

#[tokio::test]
async fn mutation_log_records_affected_entities_and_source_hashes() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-entity", "docs", "Rust async storage").await;

    let entries = store
        .read_after(GraphVersion::ZERO, 10)
        .await
        .expect("mutation log should load");

    assert_eq!(entries[0].affected_scopes, ["docs"]);
    assert_eq!(entries[0].evidence_ids, ["ev-entity"]);
    assert_eq!(entries[0].affected_entity_ids.len(), 1);
    assert_eq!(entries[0].source_hashes.len(), 1);
}

#[tokio::test]
async fn commit_receipt_counts_unique_affected_entities() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let first = EvidenceRecord::new(
        "ev-1",
        scope.clone(),
        "Rust async storage",
        vec!["Rust".to_owned()],
    )
    .expect("evidence should validate");
    let second = EvidenceRecord::new(
        "ev-2",
        scope,
        "Rust graph retrieval",
        vec!["rust".to_owned()],
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![first, second]).expect("batch should validate");

    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
    let entries = store
        .read_after(GraphVersion::ZERO, 10)
        .await
        .expect("mutation log should load");

    assert_eq!(receipt.entity_count, 1);
    assert_eq!(entries[0].entity_count, 1);
}

#[tokio::test]
async fn commits_structured_relation_claim_and_event_facts() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-structured",
        scope.clone(),
        "relay-knowledge implements BM25 retrieval",
        vec!["relay-knowledge".to_owned()],
    )
    .expect("evidence should validate");
    let relation = GraphRelationRecord::new(
        "rel-1",
        scope.clone(),
        "relay-knowledge",
        "implements",
        "BM25 retrieval",
        vec!["ev-structured".to_owned()],
    )
    .expect("relation should validate");
    let claim = ClaimRecord::new(
        "claim-1",
        scope.clone(),
        "relay-knowledge",
        "retrieval",
        "uses reciprocal-rank fusion",
        vec!["ev-structured".to_owned()],
    )
    .expect("claim should validate");
    let event = EventRecord::new(
        "event-1",
        scope,
        "index_refreshed",
        vec!["relay-knowledge".to_owned()],
        Some("2026-05-12".to_owned()),
        vec!["ev-structured".to_owned()],
    )
    .expect("event should validate");
    let batch =
        GraphMutationBatch::with_facts(vec![evidence], vec![relation], vec![claim], vec![event])
            .expect("structured graph facts should validate");

    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
    let graph = store.inspect_graph().await.expect("graph should inspect");
    let entries = store
        .read_after(GraphVersion::ZERO, 10)
        .await
        .expect("mutation log should load");

    assert_eq!(receipt.relation_count, 1);
    assert_eq!(receipt.claim_count, 1);
    assert_eq!(receipt.event_count, 1);
    assert_eq!(graph.relation_count, 1);
    assert_eq!(graph.claim_count, 1);
    assert_eq!(graph.event_count, 1);
    assert_eq!(entries[0].relation_count, 1);
    assert_eq!(entries[0].claim_count, 1);
    assert_eq!(entries[0].event_count, 1);

    let hits = store
        .search(GraphSearchRequest {
            query: "BM25".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");
    assert_eq!(hits[0].graph_facts.len(), 3);
    assert!(
        hits[0]
            .graph_facts
            .iter()
            .all(|fact| fact.version_range.valid_from == GraphVersion::new(1))
    );
}

#[tokio::test]
async fn structured_fact_references_must_match_fact_source_scope() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-docs", "docs", "Docs evidence for scoped facts").await;
    commit_evidence(
        &store,
        "ev-notes",
        "notes",
        "Notes evidence must stay scoped",
    )
    .await;

    let relation = GraphRelationRecord::new(
        "rel-cross-scope",
        SourceScope::parse("docs").expect("scope should parse"),
        "relay-knowledge",
        "uses",
        "scoped evidence",
        vec!["ev-notes".to_owned()],
    )
    .expect("relation should validate");
    let batch = GraphMutationBatch::with_facts(Vec::new(), vec![relation], Vec::new(), Vec::new())
        .expect("batch should validate");
    let error = store
        .commit_mutation_batch(batch)
        .await
        .expect_err("cross-scope evidence reference should fail");

    assert!(
        error
            .to_string()
            .contains("from source scope 'notes' instead of 'docs'")
    );
}

#[tokio::test]
async fn initialization_backfills_fact_evidence_links_for_existing_facts() {
    let path = temp_db_path("fact-links");
    {
        let store = SqliteGraphStore::open(&path).expect("store should open");
        let scope = SourceScope::parse("docs").expect("scope should parse");
        let evidence = EvidenceRecord::new(
            "ev-backfill",
            scope.clone(),
            "Backfilled structured facts remain retrievable",
            vec!["relay-knowledge".to_owned()],
        )
        .expect("evidence should validate");
        let relation = GraphRelationRecord::new(
            "rel-backfill",
            scope,
            "relay-knowledge",
            "keeps",
            "structured context",
            vec!["ev-backfill".to_owned()],
        )
        .expect("relation should validate");
        store
            .commit_mutation_batch(
                GraphMutationBatch::with_facts(
                    vec![evidence],
                    vec![relation],
                    Vec::new(),
                    Vec::new(),
                )
                .expect("batch should validate"),
            )
            .await
            .expect("commit should succeed");
        let guard = store.connection.lock().expect("connection should lock");
        guard
            .execute("DELETE FROM graph_fact_evidence", [])
            .expect("fact link gap should be simulated");
    }

    let store = SqliteGraphStore::open(&path).expect("store should reopen");
    let hits = store
        .search(GraphSearchRequest {
            query: "Backfilled".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(hits[0].graph_facts[0].fact_id, "rel-backfill");
    let _ = std::fs::remove_file(path);
}

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

#[tokio::test]
async fn search_excludes_rejected_and_superseded_evidence() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let accepted = EvidenceRecord::new(
        "ev-accepted",
        scope.clone(),
        "Lifecycle retrieval keeps accepted context",
        Vec::new(),
    )
    .expect("evidence should validate");
    let rejected = EvidenceRecord::new(
        "ev-rejected",
        scope.clone(),
        "Lifecycle rejected context must not retrieve",
        Vec::new(),
    )
    .expect("evidence should validate")
    .with_metadata(None, None, ConfidenceScore::CERTAIN, FactStatus::Rejected)
    .expect("metadata should validate");
    let superseded = EvidenceRecord::new(
        "ev-superseded",
        scope,
        "Lifecycle superseded context must not retrieve",
        Vec::new(),
    )
    .expect("evidence should validate")
    .with_metadata(None, None, ConfidenceScore::CERTAIN, FactStatus::Superseded)
    .expect("metadata should validate");
    let batch = GraphMutationBatch::new(vec![accepted, rejected, superseded])
        .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let lifecycle_hits = store
        .search(GraphSearchRequest {
            query: "Lifecycle".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");
    let rejected_hits = store
        .search(GraphSearchRequest {
            query: "rejected superseded".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(lifecycle_hits.len(), 1);
    assert_eq!(lifecycle_hits[0].evidence_id, "ev-accepted");
    assert!(rejected_hits.is_empty());
}

#[tokio::test]
async fn rejects_zero_limits_for_search_and_mutation_log() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");

    let search_error = store
        .search(GraphSearchRequest {
            query: "Rust".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::ZERO,
            limit: 0,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect_err("zero search limit should fail");
    let log_error = store
        .read_after(GraphVersion::ZERO, 0)
        .await
        .expect_err("zero log limit should fail");

    assert_eq!(
        search_error.to_string(),
        "invalid storage input: search limit must be greater than zero"
    );
    assert_eq!(
        log_error.to_string(),
        "invalid storage input: mutation log limit must be greater than zero"
    );
}

#[tokio::test]
async fn search_filters_by_source_scope_and_sorts_by_score() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Rust Rust SQLite").await;
    commit_evidence(&store, "ev-2", "repo", "Rust").await;

    let docs = store
        .search(GraphSearchRequest {
            query: "Rust SQLite".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");
    let all = store
        .search(GraphSearchRequest {
            query: "Rust".to_owned(),
            source_scope: None,
            graph_version: GraphVersion::new(2),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].source_scope, "docs");
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn search_considers_matches_beyond_newest_candidates() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-old",
        "docs",
        "Needle evidence remains searchable after newer writes",
    )
    .await;

    let scope = SourceScope::parse("docs").expect("scope should parse");
    let newer = (0..500)
        .map(|index| {
            EvidenceRecord::new(
                format!("ev-new-{index}"),
                scope.clone(),
                format!("Unrelated graph maintenance record {index}"),
                Vec::new(),
            )
            .expect("evidence should validate")
        })
        .collect::<Vec<_>>();
    let batch = GraphMutationBatch::new(newer).expect("batch should validate");
    let receipt = store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = store
        .search(GraphSearchRequest {
            query: "Needle".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: receipt.graph_version,
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].evidence_id, "ev-old");
}

#[tokio::test]
async fn search_respects_graph_version_snapshot() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Snapshot only sees Rust").await;
    commit_evidence(&store, "ev-2", "docs", "Future vector token").await;

    let before_future = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");
    let after_future = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(before_future.is_empty());
    assert_eq!(after_future.len(), 1);
    assert_eq!(after_future[0].evidence_id, "ev-2");
}

#[tokio::test]
async fn search_snapshot_excludes_updated_evidence_from_future_version() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(&store, "ev-1", "docs", "Original graph token").await;
    commit_evidence(&store, "ev-1", "docs", "Future graph token").await;

    let before_update = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(1),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");
    let after_update = store
        .search(GraphSearchRequest {
            query: "Future".to_owned(),
            source_scope: Some("docs".to_owned()),
            graph_version: GraphVersion::new(2),
            limit: 5,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed");

    assert!(before_update.is_empty());
    assert_eq!(after_update.len(), 1);
    assert_eq!(after_update[0].evidence_id, "ev-1");
}

#[test]
fn stable_entity_ids_are_deterministic() {
    assert_eq!(stable_id("entity", "Rust"), "entity:bffedf1f6f66c727");
    assert_eq!(stable_id("entity", "Rust"), stable_id("entity", "rust"));
}

#[tokio::test]
async fn open_creates_parent_database_directory() {
    let path = std::env::temp_dir()
        .join(format!("relay-knowledge-storage-{}", std::process::id()))
        .join("nested")
        .join("graph.sqlite");
    let _ = std::fs::remove_file(&path);

    let store = SqliteGraphStore::open(&path).expect("store should open");
    let version = store
        .current_graph_version()
        .await
        .expect("version should load");

    assert_eq!(version, GraphVersion::ZERO);
    assert!(path.exists());
}

async fn commit_evidence(store: &SqliteGraphStore, id: &str, source_scope: &str, content: &str) {
    let evidence = EvidenceRecord::new(
        id,
        SourceScope::parse(source_scope).expect("scope should parse"),
        content,
        vec!["Rust".to_owned()],
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");

    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
}

fn parsed_code_file(scope: &str, path: &str, symbol_id: &str) -> CodeFileRecord {
    let source_scope = SourceScope::parse(scope).expect("scope should parse");
    let extraction = code_extraction();
    let symbol = CodeSymbolRecord::new(
        symbol_id,
        source_scope.clone(),
        path,
        "main",
        CodeSymbolKind::Function,
        code_range(),
        extraction.clone(),
    )
    .expect("symbol should validate");
    let chunk = CodeChunkRecord::new(
        format!("chunk-{symbol_id}"),
        source_scope.clone(),
        path,
        "fn main() {}",
        code_range(),
        vec![symbol_id.to_owned()],
        Some(extraction),
    )
    .expect("chunk should validate");

    CodeFileRecord::new(CodeFileFields {
        source_scope,
        path: path.to_owned(),
        content_hash: format!("hash-{symbol_id}"),
        language_id: "rust".to_owned(),
        parse_status: CodeParseStatus::Parsed,
        diagnostic: None,
        symbols: vec![symbol],
        references: Vec::new(),
        chunks: vec![chunk],
    })
    .expect("file should validate")
}

fn code_extraction() -> CodeExtractionMetadata {
    CodeExtractionMetadata::new(
        "tree-sitter-rust@0.23",
        "rust-tags",
        "v1",
        "function_item",
        "definition.function",
    )
    .expect("extraction metadata should validate")
}

fn code_range() -> CodeRange {
    CodeRange::new(0, 12, 1, 1).expect("range should validate")
}

fn temp_db_path(test_name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    path.push(format!(
        "relay-knowledge-{test_name}-{}-{unique}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);

    path
}
