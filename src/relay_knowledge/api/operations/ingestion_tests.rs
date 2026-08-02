use crate::domain::{EvidenceExtractionMetadata, EvidenceModality};

use super::IngestEvidenceExtraction;

#[test]
fn extraction_conversion_supplies_the_domain_default_diagnostic() {
    let converted = IngestEvidenceExtraction {
        modality: EvidenceModality::TextSpan,
        source_uri: None,
        source_hash: None,
        media_hash: None,
        extractor: None,
        extractor_version: None,
        observed_at: None,
        parent_evidence_id: None,
        layout_region: None,
        embedding_model: None,
        embedding_dimension: None,
        diagnostic: None,
    }
    .into_domain_metadata();

    assert_eq!(
        converted.diagnostic,
        EvidenceExtractionMetadata::text_span().diagnostic
    );
}
