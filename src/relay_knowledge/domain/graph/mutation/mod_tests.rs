//! Unit contract for graph mutation facts, validation, and batch identity.

use super::*;

#[test]
fn mutation_batch_requires_graph_facts() {
    let error = GraphMutationBatch::new(Vec::new()).expect_err("empty batch should fail");

    assert_eq!(error.field, "graph_facts");
}

#[test]
fn mutation_batch_rejects_duplicate_evidence_ids() {
    let scope = SourceScope::parse("repo").expect("scope should parse");
    let first = EvidenceRecord::new("ev-1", scope.clone(), "first", Vec::new())
        .expect("evidence should validate");
    let second =
        EvidenceRecord::new("ev-1", scope, "second", Vec::new()).expect("evidence should validate");

    let error = GraphMutationBatch::new(vec![first, second]).expect_err("duplicate ID should fail");

    assert_eq!(error.field, "evidence_id");
}

#[test]
fn evidence_normalizes_duplicate_entity_labels() {
    let evidence = EvidenceRecord::new(
        "ev-1",
        SourceScope::parse("repo").expect("scope should parse"),
        "Rust owns safety guarantees",
        vec![" Rust ".to_owned(), "Rust".to_owned()],
    )
    .expect("evidence should validate");

    assert_eq!(evidence.entity_labels, ["Rust"]);
}

#[test]
fn evidence_metadata_validates_span_and_confidence() {
    let span = EvidenceSpan::new(2, 8, 1, 1).expect("span should validate");
    let confidence = ConfidenceScore::from_ratio(0.875).expect("confidence should validate");
    let evidence = EvidenceRecord::new(
        "ev-1",
        SourceScope::parse("repo").expect("scope should parse"),
        "GraphRAG context packing",
        Vec::new(),
    )
    .expect("evidence should validate")
    .with_metadata(
        Some("docs/spec.md".to_owned()),
        Some(span),
        confidence,
        FactStatus::Proposed,
    )
    .expect("metadata should validate");

    assert_eq!(evidence.source_path.as_deref(), Some("docs/spec.md"));
    assert_eq!(evidence.confidence.basis_points, 8750);
    assert_eq!(evidence.status, FactStatus::Proposed);

    let invalid = EvidenceRecord::new(
        "ev-invalid",
        SourceScope::parse("repo").expect("scope should parse"),
        "GraphRAG context packing",
        Vec::new(),
    )
    .expect("evidence should validate")
    .with_metadata(
        None,
        Some(EvidenceSpan {
            start_byte: 8,
            end_byte: 2,
            start_line: 0,
            end_line: 0,
        }),
        ConfidenceScore {
            basis_points: 10_001,
        },
        FactStatus::Accepted,
    )
    .expect_err("invalid deserialized metadata should fail");
    assert_eq!(invalid.field, "evidence_span");
}

#[test]
fn evidence_accepts_multimodal_extraction_metadata() {
    let evidence = EvidenceRecord::new(
        "ocr-1",
        SourceScope::parse("docs").expect("scope should parse"),
        "Detected diagram label",
        vec!["Diagram".to_owned()],
    )
    .expect("evidence should validate")
    .with_extraction_metadata(EvidenceExtractionMetadata {
        modality: crate::domain::EvidenceModality::OcrText,
        parent_evidence_id: Some("image-1".to_owned()),
        extractor: Some("ocr-worker".to_owned()),
        extractor_version: Some("1.0".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    })
    .expect("extraction metadata should validate");

    assert_eq!(evidence.extraction.modality.as_str(), "ocr_text");
    assert_eq!(
        evidence.extraction.parent_evidence_id.as_deref(),
        Some("image-1")
    );
}

#[test]
fn structured_facts_validate_ids_and_version_ranges() {
    let missing_evidence =
        GraphRelationRecord::new("rel-empty", scope(), "Rust", "uses", "SQLite", Vec::new())
            .expect_err("structured facts require evidence references");
    let relation = GraphRelationRecord::new(
        "rel-1",
        scope(),
        "Rust",
        "uses",
        "SQLite",
        vec!["ev-1".to_owned()],
    )
    .expect("relation should validate");
    let claim = ClaimRecord::new(
        "claim-1",
        scope(),
        "Rust",
        "supports",
        "async service boundaries",
        vec!["ev-1".to_owned()],
    )
    .expect("claim should validate");
    let event = EventRecord::new(
        "event-1",
        scope(),
        "indexed",
        vec!["Rust".to_owned()],
        Some("2026-05-12".to_owned()),
        vec!["ev-1".to_owned()],
    )
    .expect("event should validate");
    let range_error = GraphVersionRange::new(GraphVersion::new(2), Some(GraphVersion::new(1)))
        .expect_err("reversed range should fail");
    let metadata_error = relation
        .clone()
        .with_metadata(
            ConfidenceScore {
                basis_points: 10_001,
            },
            FactStatus::Accepted,
            GraphVersionRange {
                valid_from: GraphVersion::new(2),
                valid_until: Some(GraphVersion::new(1)),
            },
        )
        .expect_err("deserialized metadata should be revalidated");

    let batch =
        GraphMutationBatch::with_facts(Vec::new(), vec![relation], vec![claim], vec![event])
            .expect("structured fact batch should validate");

    assert_eq!(missing_evidence.field, "evidence_id");
    assert_eq!(range_error.field, "version_range");
    assert_eq!(metadata_error.field, "confidence");
    assert_eq!(batch.relations.len(), 1);
    assert_eq!(batch.claims.len(), 1);
    assert_eq!(batch.events.len(), 1);
}

#[test]
fn parses_fact_status_wire_values() {
    assert_eq!(
        FactStatus::parse("accepted").expect("status"),
        FactStatus::Accepted
    );
    assert_eq!(
        FactStatus::parse("mystery")
            .expect_err("unknown status should fail")
            .field,
        "fact_status"
    );
}

fn scope() -> SourceScope {
    SourceScope::parse("repo").expect("scope should parse")
}
