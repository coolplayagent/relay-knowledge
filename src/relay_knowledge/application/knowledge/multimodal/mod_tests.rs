use super::*;
use crate::api::{IngestEvidence, IngestEvidenceExtraction};

#[test]
fn converts_worker_outputs_to_ingest_request() {
    let converted = extraction_ingest_request(MultimodalExtractionRequest {
        source_scope: " docs ".to_owned(),
        parent_evidence_id: " image-1 ".to_owned(),
        derived_evidence: vec![derived_evidence(EvidenceModality::OcrText, "image-1")],
    })
    .expect("request should validate");

    assert_eq!(converted.source_scope(), "docs");
    assert_eq!(converted.parent_evidence_id, "image-1");
    assert_eq!(converted.derived_evidence_count, 1);
}

#[test]
fn rejects_query_hot_path_or_unowned_outputs() {
    let missing_metadata = IngestEvidence {
        extraction: None,
        ..derived_evidence(EvidenceModality::OcrText, "image-1")
    };
    let direct_image = derived_evidence(EvidenceModality::ImageAsset, "image-1");
    let wrong_parent = derived_evidence(EvidenceModality::Caption, "other");
    let missing_extractor = IngestEvidence {
        extraction: Some(IngestEvidenceExtraction {
            extractor: Some(" ".to_owned()),
            ..extraction(EvidenceModality::Caption, "image-1")
        }),
        ..derived_evidence(EvidenceModality::Caption, "image-1")
    };

    for evidence in [
        missing_metadata,
        direct_image,
        wrong_parent,
        missing_extractor,
    ] {
        let error = extraction_ingest_request(MultimodalExtractionRequest {
            source_scope: "docs".to_owned(),
            parent_evidence_id: "image-1".to_owned(),
            derived_evidence: vec![evidence],
        })
        .expect_err("invalid output should be rejected");

        assert!(!error.is_empty());
    }
}

impl MultimodalExtractionIngest {
    fn source_scope(&self) -> &str {
        &self.ingest.source_scope
    }
}

fn derived_evidence(modality: EvidenceModality, parent: &str) -> IngestEvidence {
    IngestEvidence {
        id: Some(format!("derived-{}", modality.as_str())),
        source_path: None,
        span: None,
        confidence: None,
        status: None,
        content: "derived multimodal content".to_owned(),
        entity_labels: Vec::new(),
        extraction: Some(extraction(modality, parent)),
    }
}

fn extraction(modality: EvidenceModality, parent: &str) -> IngestEvidenceExtraction {
    IngestEvidenceExtraction {
        modality,
        source_uri: None,
        source_hash: None,
        media_hash: None,
        extractor: Some("fixture-worker".to_owned()),
        extractor_version: Some("1.0".to_owned()),
        observed_at: None,
        parent_evidence_id: Some(parent.to_owned()),
        layout_region: None,
        embedding_model: None,
        embedding_dimension: None,
        diagnostic: None,
    }
}
