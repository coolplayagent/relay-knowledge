use crate::{
    api::{IngestEvidence, IngestRequest, MultimodalExtractionRequest},
    domain::{EvidenceModality, SourceScope},
};

const MAX_MULTIMODAL_EXTRACTION_ITEMS: usize = 64;

#[derive(Debug)]
pub(super) struct MultimodalExtractionIngest {
    pub parent_evidence_id: String,
    pub derived_evidence_count: usize,
    pub ingest: IngestRequest,
}

pub(super) fn extraction_ingest_request(
    request: MultimodalExtractionRequest,
) -> Result<MultimodalExtractionIngest, String> {
    let source_scope = SourceScope::parse(request.source_scope)
        .map(String::from)
        .map_err(|error| error.to_string())?;
    let parent_evidence_id = required_text("parent_evidence_id", request.parent_evidence_id)?;
    validate_batch_size(request.derived_evidence.len())?;
    for evidence in &request.derived_evidence {
        validate_derived_evidence(evidence, &parent_evidence_id)?;
    }

    Ok(MultimodalExtractionIngest {
        parent_evidence_id,
        derived_evidence_count: request.derived_evidence.len(),
        ingest: IngestRequest {
            source_scope,
            evidence: request.derived_evidence,
            relations: Vec::new(),
            claims: Vec::new(),
            events: Vec::new(),
        },
    })
}

fn validate_batch_size(count: usize) -> Result<(), String> {
    if count == 0 {
        return Err("multimodal extraction batch must include derived evidence".to_owned());
    }
    if count > MAX_MULTIMODAL_EXTRACTION_ITEMS {
        return Err(format!(
            "multimodal extraction batch limit is {MAX_MULTIMODAL_EXTRACTION_ITEMS} items"
        ));
    }

    Ok(())
}

fn validate_derived_evidence(
    evidence: &IngestEvidence,
    parent_evidence_id: &str,
) -> Result<(), String> {
    let extraction = evidence
        .extraction
        .as_ref()
        .ok_or_else(|| "derived multimodal evidence requires extraction metadata".to_owned())?;
    if !maintenance_modality(extraction.modality) {
        return Err(format!(
            "modality '{}' is not produced by multimodal maintenance",
            extraction.modality.as_str()
        ));
    }
    if extraction.parent_evidence_id.as_deref().map(str::trim) != Some(parent_evidence_id) {
        return Err(format!(
            "derived evidence must reference parent evidence '{parent_evidence_id}'"
        ));
    }
    if extraction
        .extractor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err("derived multimodal evidence requires extractor identity".to_owned());
    }

    Ok(())
}

fn maintenance_modality(modality: EvidenceModality) -> bool {
    matches!(
        modality,
        EvidenceModality::OcrText
            | EvidenceModality::Caption
            | EvidenceModality::ImageEmbedding
            | EvidenceModality::Table
            | EvidenceModality::LayoutRegion
    )
}

fn required_text(field: &'static str, value: String) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} must not be empty"));
    }

    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
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
}
