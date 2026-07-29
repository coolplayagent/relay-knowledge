use super::*;

#[test]
fn validates_derived_multimodal_metadata() {
    let metadata = EvidenceExtractionMetadata {
        modality: EvidenceModality::OcrText,
        parent_evidence_id: Some(" image-1 ".to_owned()),
        extractor: Some(" tesseract ".to_owned()),
        extractor_version: Some(" 5.4 ".to_owned()),
        observed_at: Some(" 2026-05-13T00:00:00Z ".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    }
    .validate()
    .expect("metadata should validate");

    assert_eq!(metadata.parent_evidence_id.as_deref(), Some("image-1"));
    assert_eq!(metadata.extractor.as_deref(), Some("tesseract"));
}

#[test]
fn rejects_incomplete_multimodal_metadata() {
    let derived = EvidenceExtractionMetadata {
        modality: EvidenceModality::Caption,
        ..EvidenceExtractionMetadata::text_span()
    }
    .validate()
    .expect_err("derived evidence needs a parent");
    let failed =
        ExtractionDiagnostic::new(ExtractionStatus::Failed, None).expect_err("message needed");
    let layout = EvidenceExtractionMetadata {
        modality: EvidenceModality::LayoutRegion,
        parent_evidence_id: Some("page-1".to_owned()),
        ..EvidenceExtractionMetadata::text_span()
    }
    .validate()
    .expect_err("layout coordinates needed");
    let invalid_region = EvidenceExtractionMetadata {
        modality: EvidenceModality::LayoutRegion,
        parent_evidence_id: Some("page-1".to_owned()),
        layout_region: Some(LayoutRegion {
            page_number: 0,
            x: 0,
            y: 0,
            width: 0,
            height: 1,
        }),
        ..EvidenceExtractionMetadata::text_span()
    }
    .validate()
    .expect_err("layout region invariants should be rechecked");
    let empty_embedding = EvidenceExtractionMetadata {
        modality: EvidenceModality::ImageEmbedding,
        parent_evidence_id: Some("image-1".to_owned()),
        embedding_model: Some("clip".to_owned()),
        embedding_dimension: Some(0),
        ..EvidenceExtractionMetadata::text_span()
    }
    .validate()
    .expect_err("zero dimension should be rejected");
    let degraded_without_message = EvidenceExtractionMetadata {
        diagnostic: ExtractionDiagnostic {
            status: ExtractionStatus::Degraded,
            message: Some(" ".to_owned()),
        },
        ..EvidenceExtractionMetadata::text_span()
    }
    .validate()
    .expect_err("diagnostic message invariant should be rechecked");

    assert_eq!(derived.field, "parent_evidence_id");
    assert_eq!(failed.field, "extraction_diagnostic");
    assert_eq!(layout.field, "layout_region");
    assert_eq!(invalid_region.field, "layout_region");
    assert_eq!(empty_embedding.field, "embedding_model");
    assert_eq!(degraded_without_message.field, "extraction_diagnostic");
}

#[test]
fn validates_layout_and_image_embedding_contracts() {
    let region = LayoutRegion::new(1, 10, 20, 300, 120).expect("region should validate");
    let embedding = EvidenceExtractionMetadata {
        modality: EvidenceModality::ImageEmbedding,
        parent_evidence_id: Some("image-1".to_owned()),
        embedding_model: Some("clip-local-hash-v1".to_owned()),
        embedding_dimension: Some(16),
        ..EvidenceExtractionMetadata::text_span()
    }
    .validate()
    .expect("embedding metadata should validate");

    assert_eq!(region.width, 300);
    assert_eq!(embedding.modality.as_str(), "image_embedding");
}
