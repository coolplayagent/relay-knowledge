use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::{
    api::{IngestEvidence, IngestEvidenceExtraction, IngestRequest},
    domain::{
        EvidenceModality, ExtractionDiagnostic, ExtractionStatus, FactStatus, ProposalKind,
        ProposalProvenance, WorkerKind, WorkerTaskRecord,
    },
    storage::NewProposal,
};

pub(super) fn worker_request_payload(
    task: &WorkerTaskRecord,
    request_timeout_ms: u64,
    lease_ms: u64,
    max_attempts: u32,
    max_in_flight: usize,
) -> serde_json::Value {
    json!({
        "contract_version": 2,
        "task": task,
        "proposal_policy": {
            "requires_manual_review": true,
            "structured_facts_default_status": "proposed",
            "accepted_facts_must_be_user_approved": true,
        },
        "budgets": {
            "request_timeout_ms": request_timeout_ms,
            "lease_ms": lease_ms,
            "max_attempts": max_attempts,
            "max_in_flight": max_in_flight,
        },
        "required_provenance": [
            "producer",
            "schema_version",
            "provider_or_model_when_model_assisted",
            "prompt_id_and_prompt_version_when_prompted",
        ],
    })
}

pub(super) fn fallback_proposal(
    task: &WorkerTaskRecord,
    lease_ms: u64,
    max_attempts: u32,
) -> Result<NewProposal, String> {
    let evidence_id = task
        .evidence_id
        .clone()
        .unwrap_or_else(|| task.task_id.clone());
    let derived_id = format!(
        "derived:{}:{:016x}",
        task.kind.as_str(),
        stable_hash64(task.task_id.as_bytes())
    );
    let modality = match task.kind {
        WorkerKind::Embedding => EvidenceModality::ImageEmbedding,
        WorkerKind::Ocr => EvidenceModality::OcrText,
        WorkerKind::Vision => EvidenceModality::Caption,
        WorkerKind::Extractor => EvidenceModality::LayoutRegion,
    };
    let extraction = IngestEvidenceExtraction {
        modality,
        source_uri: None,
        source_hash: None,
        media_hash: None,
        extractor: Some(format!("{}-fallback", task.kind.as_str())),
        extractor_version: Some("1".to_owned()),
        observed_at: Some(format!("{}", now_millis())),
        parent_evidence_id: Some(evidence_id.clone()),
        layout_region: (modality == EvidenceModality::LayoutRegion).then_some(
            crate::domain::LayoutRegion {
                page_number: 1,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        ),
        embedding_model: (modality == EvidenceModality::ImageEmbedding)
            .then_some("deterministic-fallback-v1".to_owned()),
        embedding_dimension: (modality == EvidenceModality::ImageEmbedding).then_some(16),
        diagnostic: Some(ExtractionDiagnostic {
            status: ExtractionStatus::Degraded,
            message: Some(
                "deterministic fallback proposal; external backend not required".to_owned(),
            ),
        }),
    };
    let request = IngestRequest {
        source_scope: task.source_scope.clone(),
        evidence: vec![IngestEvidence {
            id: Some(derived_id),
            source_path: None,
            span: None,
            confidence: None,
            status: Some(FactStatus::Accepted),
            content: format!(
                "{} fallback output for parent evidence {}",
                task.kind.as_str(),
                evidence_id
            ),
            entity_labels: Vec::new(),
            extraction: Some(extraction),
        }],
        relations: Vec::new(),
        claims: Vec::new(),
        events: Vec::new(),
    };
    let payload_json = serde_json::to_string(&request).map_err(|error| error.to_string())?;

    Ok(NewProposal {
        proposal_id: format!(
            "proposal:{}:{:016x}",
            task.kind.as_str(),
            stable_hash64(task.task_id.as_bytes())
        ),
        source_scope: task.source_scope.clone(),
        kind: ProposalKind::Evidence,
        title: format!("{} worker proposal", task.kind.as_str()),
        summary: format!("Derived evidence proposal for parent evidence {evidence_id}"),
        payload_json,
        origin: format!("worker:{}", task.kind.as_str()),
        provenance: fallback_provenance(task, &evidence_id, lease_ms, max_attempts),
        confidence_basis_points: 5_000,
        conflicts: Vec::new(),
        now_ms: now_millis(),
    })
}

pub(super) fn proposal_from_worker_response(
    task: &WorkerTaskRecord,
    value: serde_json::Value,
    lease_ms: u64,
    max_attempts: u32,
) -> Result<NewProposal, String> {
    let ingest_value = value
        .get("ingest_request")
        .cloned()
        .ok_or_else(|| "missing ingest_request".to_owned())?;
    let mut ingest =
        serde_json::from_value::<IngestRequest>(ingest_value).map_err(|error| error.to_string())?;
    apply_worker_fact_policy(task.kind, &mut ingest);
    let kind = classify_proposal_kind(&ingest);
    let payload_json = serde_json::to_string(&ingest).map_err(|error| error.to_string())?;
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("External worker proposal")
        .to_owned();
    let summary = value
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("External worker returned a graph mutation proposal")
        .to_owned();
    let confidence = value
        .get("confidence_basis_points")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(7_500);
    let provenance = value
        .get("provenance")
        .map(|metadata| {
            serde_json::from_value::<ProposalProvenance>(metadata.clone())
                .map_err(|error| error.to_string())?
                .validate()
                .map_err(|error| error.to_string())
        })
        .transpose()?
        .unwrap_or_else(|| worker_response_provenance(task, lease_ms, max_attempts));

    Ok(NewProposal {
        proposal_id: format!(
            "proposal:{}:{:016x}",
            task.kind.as_str(),
            stable_hash64(payload_json.as_bytes())
        ),
        source_scope: task.source_scope.clone(),
        kind,
        title,
        summary,
        payload_json,
        origin: format!("worker:{}", task.kind.as_str()),
        provenance,
        confidence_basis_points: confidence.min(10_000),
        conflicts: Vec::new(),
        now_ms: now_millis(),
    })
}

fn fallback_provenance(
    task: &WorkerTaskRecord,
    evidence_id: &str,
    lease_ms: u64,
    max_attempts: u32,
) -> ProposalProvenance {
    let model = match task.kind {
        WorkerKind::Embedding => Some("deterministic-fallback-v1".to_owned()),
        WorkerKind::Ocr | WorkerKind::Vision | WorkerKind::Extractor => None,
    };

    ProposalProvenance {
        producer: "deterministic_fallback".to_owned(),
        provider: Some("internal".to_owned()),
        model,
        prompt_id: Some("relay.worker.fallback".to_owned()),
        prompt_version: Some("1".to_owned()),
        schema_version: Some("worker-proposal.v2".to_owned()),
        input_source_hash: Some(task.input_fingerprint.clone()),
        input_fact_ids: vec![evidence_id.to_owned()],
        stale_when: stale_conditions(task),
        budget_notes: budget_notes(lease_ms, max_attempts),
    }
}

fn worker_response_provenance(
    task: &WorkerTaskRecord,
    lease_ms: u64,
    max_attempts: u32,
) -> ProposalProvenance {
    ProposalProvenance {
        producer: format!("worker:{}", task.kind.as_str()),
        provider: Some("configured_endpoint".to_owned()),
        model: None,
        prompt_id: None,
        prompt_version: None,
        schema_version: Some("worker-proposal.v2".to_owned()),
        input_source_hash: Some(task.input_fingerprint.clone()),
        input_fact_ids: task.evidence_id.clone().into_iter().collect(),
        stale_when: stale_conditions(task),
        budget_notes: budget_notes(lease_ms, max_attempts),
    }
}

fn stale_conditions(task: &WorkerTaskRecord) -> Vec<String> {
    vec![
        format!(
            "graph_version advances beyond {} before proposal decision",
            task.target_graph_version.get()
        ),
        "parent evidence is rejected or superseded".to_owned(),
    ]
}

fn budget_notes(lease_ms: u64, max_attempts: u32) -> Vec<String> {
    vec![
        format!("lease_ms={lease_ms}"),
        format!("max_attempts={max_attempts}"),
    ]
}

fn apply_worker_fact_policy(kind: WorkerKind, ingest: &mut IngestRequest) {
    if kind != WorkerKind::Extractor {
        return;
    }

    for relation in &mut ingest.relations {
        relation.status = Some(FactStatus::Proposed);
    }
    for claim in &mut ingest.claims {
        claim.status = Some(FactStatus::Proposed);
    }
    for event in &mut ingest.events {
        event.status = Some(FactStatus::Proposed);
    }
}

fn classify_proposal_kind(ingest: &IngestRequest) -> ProposalKind {
    if !ingest.relations.is_empty() {
        ProposalKind::Relation
    } else if !ingest.claims.is_empty() {
        ProposalKind::Claim
    } else if !ingest.events.is_empty() {
        ProposalKind::Event
    } else {
        ProposalKind::Evidence
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    hash
}
