use super::*;
use crate::domain::{
    ConfidenceScore, EventRecord, EvidenceExtractionMetadata, EvidenceModality, EvidenceRecord,
    FactStatus, GraphRelationRecord, GraphVersion, GraphVersionRange, RetrieverSource, SourceScope,
};
use crate::storage::GraphSearchRequest;

#[tokio::test]
async fn semantic_and_vector_read_models_contribute_ranked_hits() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    commit_evidence(
        &store,
        "ev-semantic",
        "docs",
        "Semantic vector metadata keeps source hash and model details",
        vec!["Semantic".to_owned(), "Vector".to_owned()],
    )
    .await;

    let hits = search(&store, "vector metadata").await;

    assert_eq!(hits[0].evidence_id, "ev-semantic");
    assert!(
        hits[0]
            .retriever_sources
            .contains(&RetrieverSource::Semantic)
    );
    assert!(hits[0].retriever_sources.contains(&RetrieverSource::Vector));
    assert!(
        hits[0]
            .ranking
            .iter()
            .any(|signal| signal.explanation.contains("semantic read model"))
    );
    assert!(
        hits[0]
            .ranking
            .iter()
            .any(|signal| signal.explanation.contains("vector ANN read model"))
    );
}

#[tokio::test]
async fn graph_path_temporal_and_community_retrieval_are_queryable() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-path",
        scope.clone(),
        "Rust and SQLite support GraphRAG path retrieval",
        vec!["Rust".to_owned(), "SQLite".to_owned()],
    )
    .expect("evidence should validate");
    let relation = GraphRelationRecord::new(
        "rel-rust-sqlite",
        scope.clone(),
        "Rust",
        "uses",
        "SQLite",
        vec!["ev-path".to_owned()],
    )
    .expect("relation should validate");
    let event = EventRecord::new(
        "event-release",
        scope,
        "released",
        vec!["Rust".to_owned()],
        Some("2026-05-13".to_owned()),
        vec!["ev-path".to_owned()],
    )
    .expect("event should validate");
    let batch =
        GraphMutationBatch::with_facts(vec![evidence], vec![relation], Vec::new(), vec![event])
            .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let path = search(&store, "uses SQLite path").await;
    let temporal = search(&store, "timeline 2026 Rust").await;
    let community = search(&store, "community summary docs").await;

    assert!(
        path[0]
            .retriever_sources
            .contains(&RetrieverSource::GraphPath)
    );
    assert!(
        temporal[0]
            .retriever_sources
            .contains(&RetrieverSource::Temporal)
    );
    assert!(community.iter().any(|hit| {
        hit.retriever_sources
            .contains(&RetrieverSource::CommunitySummary)
    }));
}

#[tokio::test]
async fn graph_path_scores_all_supporting_evidence() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let first = EvidenceRecord::new(
        "ev-path-first",
        scope.clone(),
        "First support has neutral context",
        vec!["GraphRAG".to_owned()],
    )
    .expect("first evidence should validate");
    let second = EvidenceRecord::new(
        "ev-path-second",
        scope.clone(),
        "Second support contains late matching retriever term",
        vec!["Retriever".to_owned()],
    )
    .expect("second evidence should validate");
    let relation = GraphRelationRecord::new(
        "rel-multi-support",
        scope,
        "GraphRAG",
        "uses",
        "Retriever",
        vec!["ev-path-first".to_owned(), "ev-path-second".to_owned()],
    )
    .expect("relation should validate");
    let batch =
        GraphMutationBatch::with_facts(vec![first, second], vec![relation], Vec::new(), Vec::new())
            .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = search(&store, "late matching retriever").await;

    assert!(
        hits.iter()
            .any(|hit| { hit.retriever_sources.contains(&RetrieverSource::GraphPath) })
    );
}

#[tokio::test]
async fn community_summary_respects_fact_and_evidence_status() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let accepted = EvidenceRecord::new(
        "ev-community-accepted",
        scope.clone(),
        "Accepted community context",
        vec!["AcceptedEntity".to_owned()],
    )
    .expect("accepted evidence should validate");
    let rejected = EvidenceRecord::new(
        "ev-community-rejected",
        scope.clone(),
        "Rejected community context",
        vec!["RejectedOnly".to_owned()],
    )
    .expect("rejected evidence should validate")
    .with_metadata(None, None, ConfidenceScore::CERTAIN, FactStatus::Rejected)
    .expect("rejected metadata should validate");
    let accepted_relation = GraphRelationRecord::new(
        "rel-community-accepted",
        scope.clone(),
        "AcceptedEntity",
        "uses",
        "GraphRAG",
        vec!["ev-community-accepted".to_owned()],
    )
    .expect("accepted relation should validate");
    let rejected_relation = GraphRelationRecord::new(
        "rel-community-rejected",
        scope,
        "AcceptedEntity",
        "mentions",
        "RejectedOnly",
        vec!["ev-community-accepted".to_owned()],
    )
    .expect("rejected relation should validate")
    .with_metadata(
        ConfidenceScore::CERTAIN,
        FactStatus::Rejected,
        GraphVersionRange::open_from(GraphVersion::ZERO),
    )
    .expect("rejected relation metadata should validate");
    let batch = GraphMutationBatch::with_facts(
        vec![accepted, rejected],
        vec![accepted_relation, rejected_relation],
        Vec::new(),
        Vec::new(),
    )
    .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = search(&store, "community summary docs").await;
    let summary = hits
        .iter()
        .find(|hit| {
            hit.retriever_sources
                .contains(&RetrieverSource::CommunitySummary)
        })
        .expect("community summary should be returned");

    assert!(summary.content.contains("relations 1"));
    assert!(!summary.content.contains("RejectedOnly"));
}

#[tokio::test]
async fn community_summary_uses_versioned_evidence_scope_for_counts() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let docs = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-moved-scope",
        docs.clone(),
        "Evidence before scope move",
        vec!["MovedEntity".to_owned()],
    )
    .expect("evidence should validate");
    let relation = GraphRelationRecord::new(
        "rel-before-move",
        docs,
        "MovedEntity",
        "uses",
        "GraphRAG",
        vec!["ev-moved-scope".to_owned()],
    )
    .expect("relation should validate");
    store
        .commit_mutation_batch(
            GraphMutationBatch::with_facts(vec![evidence], vec![relation], Vec::new(), Vec::new())
                .expect("batch should validate"),
        )
        .await
        .expect("initial commit should succeed");
    let moved = EvidenceRecord::new(
        "ev-moved-scope",
        SourceScope::parse("other").expect("scope should parse"),
        "Evidence after scope move",
        vec!["MovedEntity".to_owned()],
    )
    .expect("moved evidence should validate");
    store
        .commit_mutation_batch(GraphMutationBatch::new(vec![moved]).expect("batch"))
        .await
        .expect("move commit should succeed");

    let hits = search_at(
        &store,
        "community summary other",
        Some("other".to_owned()),
        GraphVersion::new(1),
    )
    .await;
    let summary = hits
        .iter()
        .find(|hit| {
            hit.retriever_sources
                .contains(&RetrieverSource::CommunitySummary)
        })
        .expect("community summary should be returned");

    assert!(summary.content.contains("relations 0"));
}

#[tokio::test]
async fn path_and_temporal_retrieval_respect_fact_validity_windows() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let evidence = EvidenceRecord::new(
        "ev-validity",
        scope.clone(),
        "Neutral supporting context",
        vec!["SupportOnly".to_owned()],
    )
    .expect("evidence should validate");
    let future_relation = GraphRelationRecord::new(
        "rel-future",
        scope.clone(),
        "FutureSource",
        "depends_on",
        "FutureTarget",
        vec!["ev-validity".to_owned()],
    )
    .expect("relation should validate")
    .with_metadata(
        ConfidenceScore::CERTAIN,
        FactStatus::Accepted,
        GraphVersionRange::open_from(GraphVersion::new(2)),
    )
    .expect("future relation metadata should validate");
    let future_event = EventRecord::new(
        "event-future",
        scope,
        "planned",
        vec!["FutureEvent".to_owned()],
        Some("2026-05-13".to_owned()),
        vec!["ev-validity".to_owned()],
    )
    .expect("event should validate")
    .with_metadata(
        ConfidenceScore::CERTAIN,
        FactStatus::Accepted,
        GraphVersionRange::open_from(GraphVersion::new(2)),
    )
    .expect("future event metadata should validate");
    let batch = GraphMutationBatch::with_facts(
        vec![evidence],
        vec![future_relation],
        Vec::new(),
        vec![future_event],
    )
    .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = search(&store, "timeline FutureSource FutureEvent").await;

    assert!(!hits.iter().any(|hit| {
        hit.retriever_sources.contains(&RetrieverSource::GraphPath)
            || hit.retriever_sources.contains(&RetrieverSource::Temporal)
    }));
}

#[tokio::test]
async fn multimodal_derived_evidence_is_grouped_by_parent() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let image = EvidenceRecord::new(
        "image-1",
        scope.clone(),
        "Architecture diagram image asset",
        vec!["GraphRAG".to_owned()],
    )
    .expect("image evidence should validate")
    .with_extraction_metadata(EvidenceExtractionMetadata {
        modality: EvidenceModality::ImageAsset,
        media_hash: Some("sha256:image".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    })
    .expect("image metadata should validate");
    let ocr = EvidenceRecord::new(
        "ocr-1",
        scope.clone(),
        "OCR text says vector ANN read model",
        vec!["Vector".to_owned()],
    )
    .expect("ocr evidence should validate")
    .with_extraction_metadata(EvidenceExtractionMetadata {
        modality: EvidenceModality::OcrText,
        parent_evidence_id: Some("image-1".to_owned()),
        extractor: Some("ocr-worker".to_owned()),
        extractor_version: Some("1.0".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    })
    .expect("ocr metadata should validate");
    let caption = EvidenceRecord::new(
        "caption-1",
        scope.clone(),
        "Caption describes GraphRAG architecture",
        vec!["GraphRAG".to_owned()],
    )
    .expect("caption evidence should validate")
    .with_extraction_metadata(EvidenceExtractionMetadata {
        modality: EvidenceModality::Caption,
        parent_evidence_id: Some("image-1".to_owned()),
        extractor: Some("caption-worker".to_owned()),
        extractor_version: Some("1.0".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    })
    .expect("caption metadata should validate");
    let relation = GraphRelationRecord::new(
        "rel-ocr-vector",
        scope,
        "Image OCR",
        "mentions",
        "Vector",
        vec!["ocr-1".to_owned()],
    )
    .expect("relation should validate");
    let batch = GraphMutationBatch::with_facts(
        vec![image, ocr, caption],
        vec![relation],
        Vec::new(),
        Vec::new(),
    )
    .expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");

    let hits = search(&store, "GraphRAG OCR vector").await;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].evidence_id, "image-1");
    assert!(hits[0].content.contains("OCR text says vector"));
    assert!(hits[0].content.contains("Caption describes GraphRAG"));
    assert!(
        hits[0]
            .graph_facts
            .iter()
            .any(|fact| fact.fact_id == "rel-ocr-vector" && fact.evidence_ids == ["ocr-1"])
    );
}

#[tokio::test]
async fn multimodal_parent_evidence_must_exist_in_same_scope() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let scope = SourceScope::parse("docs").expect("scope should parse");
    let child = EvidenceRecord::new(
        "ocr-missing-parent",
        scope.clone(),
        "OCR text with no image parent",
        vec!["OCR".to_owned()],
    )
    .expect("child evidence should validate")
    .with_extraction_metadata(EvidenceExtractionMetadata {
        modality: EvidenceModality::OcrText,
        parent_evidence_id: Some("missing-image".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    })
    .expect("child metadata should validate");
    let missing_parent = store
        .commit_mutation_batch(GraphMutationBatch::new(vec![child]).expect("batch"))
        .await
        .expect_err("missing parent should be rejected");

    assert!(missing_parent.to_string().contains("missing-image"));

    let other_scope = SourceScope::parse("other").expect("scope should parse");
    let parent = EvidenceRecord::new(
        "image-other",
        other_scope,
        "Architecture diagram in another scope",
        vec!["Image".to_owned()],
    )
    .expect("parent evidence should validate")
    .with_extraction_metadata(EvidenceExtractionMetadata {
        modality: EvidenceModality::ImageAsset,
        media_hash: Some("sha256:other-image".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    })
    .expect("parent metadata should validate");
    let child = EvidenceRecord::new(
        "ocr-wrong-scope",
        scope,
        "OCR text points at another scope",
        vec!["OCR".to_owned()],
    )
    .expect("child evidence should validate")
    .with_extraction_metadata(EvidenceExtractionMetadata {
        modality: EvidenceModality::OcrText,
        parent_evidence_id: Some("image-other".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    })
    .expect("child metadata should validate");
    let wrong_scope = store
        .commit_mutation_batch(GraphMutationBatch::new(vec![parent, child]).expect("batch"))
        .await
        .expect_err("cross-scope parent should be rejected");

    assert!(wrong_scope.to_string().contains("instead of 'docs'"));
}

async fn commit_evidence(
    store: &SqliteGraphStore,
    id: &str,
    source_scope: &str,
    content: &str,
    entity_labels: Vec<String>,
) {
    let evidence = EvidenceRecord::new(
        id,
        SourceScope::parse(source_scope).expect("scope should parse"),
        content,
        entity_labels,
    )
    .expect("evidence should validate");
    let batch = GraphMutationBatch::new(vec![evidence]).expect("batch should validate");
    store
        .commit_mutation_batch(batch)
        .await
        .expect("commit should succeed");
}

async fn search(store: &SqliteGraphStore, query: &str) -> Vec<RetrievalHit> {
    search_at(store, query, Some("docs".to_owned()), GraphVersion::new(1)).await
}

async fn search_at(
    store: &SqliteGraphStore,
    query: &str,
    source_scope: Option<String>,
    graph_version: GraphVersion,
) -> Vec<RetrievalHit> {
    store
        .search(GraphSearchRequest {
            query: query.to_owned(),
            source_scope,
            graph_version,
            limit: 10,
            disabled_retriever_sources: Vec::new(),
        })
        .await
        .expect("search should succeed")
}
