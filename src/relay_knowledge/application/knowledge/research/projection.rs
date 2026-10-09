//! Converts validated bundles to explicitly proposed graph facts.
use super::bundle::{LoadedBundle, document_id};
use crate::{
    api::{
        ApiError, IngestClaim, IngestEvidence, IngestEvidenceExtraction, IngestRelation,
        IngestRequest,
    },
    domain::FactStatus,
};

pub(super) fn ingest_request(loaded: &LoadedBundle) -> Result<IngestRequest, ApiError> {
    let bundle = &loaded.bundle;
    let revision = &loaded.report.bundle_sha256;
    let document = document_id(&bundle.id, &bundle.source_scope, revision);
    let entity = |id: &str| {
        format!(
            "{}:{}",
            document_id(&bundle.id, &bundle.source_scope, "node"),
            super::reader::digest(id.as_bytes())
        )
    };
    let pin_id = |id: &str| format!("{document}:pin:{}", super::reader::digest(id.as_bytes()));
    let payload = serde_json::to_string(bundle)
        .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
    let mut evidence = vec![IngestEvidence {
        id: Some(document.clone()),
        source_path: None,
        span: None,
        confidence: None,
        status: Some(FactStatus::Proposed),
        content: payload,
        entity_labels: bundle
            .graph
            .nodes
            .iter()
            .map(|node| entity(&node.id))
            .collect(),
        extraction: None,
    }];
    for pin in &bundle.evidence {
        let content = loaded
            .snippets
            .get(&pin.id)
            .ok_or_else(|| ApiError::invalid_argument("cannot project unverified evidence"))?;
        evidence.push(IngestEvidence {
            id: Some(pin_id(&pin.id)),
            source_path: loaded.source_paths.get(&pin.id).cloned(),
            span: pin.span,
            confidence: None,
            status: Some(FactStatus::Proposed),
            content: content.clone(),
            entity_labels: Vec::new(),
            extraction: Some(IngestEvidenceExtraction {
                modality: crate::domain::EvidenceModality::TextSpan,
                source_uri: loaded.source_paths.get(&pin.id).cloned(),
                source_hash: Some(pin.artifact.sha256.clone()),
                media_hash: None,
                extractor: Some("authored-evidence-bundle".into()),
                extractor_version: Some("1".into()),
                observed_at: None,
                parent_evidence_id: Some(document.clone()),
                layout_region: None,
                embedding_model: None,
                embedding_dimension: None,
                diagnostic: None,
            }),
        });
    }
    let relations = bundle
        .graph
        .edges
        .iter()
        .zip(&loaded.report.relations)
        .map(|(edge, audit)| IngestRelation {
            id: format!(
                "{document}:relation:{}",
                super::reader::digest(audit.id.as_bytes())
            ),
            source_entity_label: entity(&edge.source),
            relation_type: edge.relation.clone(),
            target_entity_label: entity(&edge.target),
            evidence_ids: edge.evidence.iter().map(|id| pin_id(id)).collect(),
            confidence: None,
            status: Some(FactStatus::Proposed),
            version_range: None,
        })
        .collect();
    let mut claims: Vec<_> = bundle
        .graph
        .nodes
        .iter()
        .map(|node| IngestClaim {
            id: format!(
                "{document}:label:{}",
                super::reader::digest(node.id.as_bytes())
            ),
            subject_entity_label: entity(&node.id),
            predicate: "authored_label".into(),
            object: node.label.clone(),
            evidence_ids: vec![document.clone()],
            confidence: None,
            status: Some(FactStatus::Proposed),
            version_range: None,
        })
        .collect();
    if let Some(previous) = &bundle.supersedes {
        claims.push(IngestClaim {
            id: format!("{document}:supersession"),
            subject_entity_label: document_id(&bundle.id, &bundle.source_scope, "identity"),
            predicate: "proposes_supersession".into(),
            object: previous.clone(),
            evidence_ids: vec![document],
            confidence: None,
            status: Some(FactStatus::Proposed),
            version_range: None,
        });
    }
    Ok(IngestRequest {
        source_scope: bundle.source_scope.clone(),
        evidence,
        relations,
        claims,
        events: Vec::new(),
    })
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
