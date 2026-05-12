use crate::{
    api::IngestRequest,
    domain::{
        ClaimRecord, ConfidenceScore, DomainError, EventRecord, EvidenceRecord, EvidenceSpan,
        FactStatus, GraphMutationBatch, GraphRelationRecord, GraphVersion, GraphVersionRange,
        SourceScope,
    },
};

pub(super) fn mutation_batch_from_request(
    request: IngestRequest,
) -> Result<GraphMutationBatch, DomainError> {
    let source_scope = SourceScope::parse(request.source_scope)?;
    let mut records = Vec::with_capacity(request.evidence.len());
    for evidence in request.evidence {
        let content = evidence.content.trim().to_owned();
        let source_path = evidence.source_path.map(|path| path.trim().to_owned());
        let span = evidence.span;
        let id = evidence.id.unwrap_or_else(|| {
            generated_evidence_id(
                source_scope.as_str(),
                source_path.as_deref(),
                span,
                &content,
            )
        });
        let record =
            EvidenceRecord::new(id, source_scope.clone(), content, evidence.entity_labels)?
                .with_metadata(
                    source_path,
                    span,
                    evidence.confidence.unwrap_or(ConfidenceScore::CERTAIN),
                    evidence.status.unwrap_or(FactStatus::Accepted),
                )?;
        records.push(record);
    }

    let relations = request
        .relations
        .into_iter()
        .map(|relation| {
            GraphRelationRecord::new(
                relation.id,
                source_scope.clone(),
                relation.source_entity_label,
                relation.relation_type,
                relation.target_entity_label,
                relation.evidence_ids,
            )
            .and_then(|record| {
                record.with_metadata(
                    relation.confidence.unwrap_or(ConfidenceScore::CERTAIN),
                    relation.status.unwrap_or(FactStatus::Accepted),
                    relation
                        .version_range
                        .unwrap_or(GraphVersionRange::open_from(GraphVersion::ZERO)),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let claims = request
        .claims
        .into_iter()
        .map(|claim| {
            ClaimRecord::new(
                claim.id,
                source_scope.clone(),
                claim.subject_entity_label,
                claim.predicate,
                claim.object,
                claim.evidence_ids,
            )
            .and_then(|record| {
                record.with_metadata(
                    claim.confidence.unwrap_or(ConfidenceScore::CERTAIN),
                    claim.status.unwrap_or(FactStatus::Accepted),
                    claim
                        .version_range
                        .unwrap_or(GraphVersionRange::open_from(GraphVersion::ZERO)),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let events = request
        .events
        .into_iter()
        .map(|event| {
            EventRecord::new(
                event.id,
                source_scope.clone(),
                event.event_type,
                event.entity_labels,
                event.occurred_at,
                event.evidence_ids,
            )
            .and_then(|record| {
                record.with_metadata(
                    event.confidence.unwrap_or(ConfidenceScore::CERTAIN),
                    event.status.unwrap_or(FactStatus::Accepted),
                    event
                        .version_range
                        .unwrap_or(GraphVersionRange::open_from(GraphVersion::ZERO)),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    GraphMutationBatch::with_facts(records, relations, claims, events)
}

pub(super) fn generated_evidence_id(
    scope: &str,
    source_path: Option<&str>,
    span: Option<EvidenceSpan>,
    content: &str,
) -> String {
    let metadata_len = source_path.map(str::len).unwrap_or_default() + usize::from(span.is_some());
    let mut input = Vec::with_capacity(scope.len() + content.len() + metadata_len + 64);
    input.extend_from_slice(&(scope.len() as u64).to_le_bytes());
    input.extend_from_slice(scope.as_bytes());
    input.extend_from_slice(&(content.len() as u64).to_le_bytes());
    input.extend_from_slice(content.as_bytes());
    if source_path.is_some() || span.is_some() {
        input.extend_from_slice(&(source_path.unwrap_or_default().len() as u64).to_le_bytes());
        input.extend_from_slice(source_path.unwrap_or_default().as_bytes());
        match span {
            Some(span) => {
                input.extend_from_slice(&span.start_byte.to_le_bytes());
                input.extend_from_slice(&span.end_byte.to_le_bytes());
                input.extend_from_slice(&span.start_line.to_le_bytes());
                input.extend_from_slice(&span.end_line.to_le_bytes());
            }
            None => input.extend_from_slice(&[0; 16]),
        }
    }

    format!("evidence:{:016x}", stable_hash64(&input))
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
