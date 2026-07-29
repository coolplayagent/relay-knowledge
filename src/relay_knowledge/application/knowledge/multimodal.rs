use crate::{
    api::{IngestEvidence, IngestRequest, MultimodalExtractionRequest},
    domain::{EvidenceModality, SourceScope},
};

const MAX_MULTIMODAL_EXTRACTION_ITEMS: usize = 64;

#[derive(Debug)]
pub(in crate::application) struct MultimodalExtractionIngest {
    pub parent_evidence_id: String,
    pub derived_evidence_count: usize,
    pub ingest: IngestRequest,
}

pub(in crate::application) fn extraction_ingest_request(
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
#[path = "multimodal_tests.rs"]
mod tests;
