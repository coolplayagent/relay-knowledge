use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::{
    domain::{ContextGraphFact, GraphVersion, RetrievalHit, RetrieverSource},
    storage::StorageError,
};

use super::{
    super::{
        bm25::RawBm25Row,
        context::{code_artifact_for_document, entities_for_evidence, evidence_span},
    },
    candidate::{ScoredHit, evidence_group_key},
};

pub(in crate::storage::sqlite::retrieval) fn scored_bm25_hit(
    connection: &Connection,
    row: RawBm25Row,
    graph_version: GraphVersion,
    facts_by_evidence: &BTreeMap<String, Vec<ContextGraphFact>>,
) -> Result<ScoredHit, StorageError> {
    let source = match row.document_kind.as_str() {
        "code_symbol" | "code_chunk" => RetrieverSource::CodeGraph,
        _ => RetrieverSource::Bm25,
    };
    let (source_span, entities, graph_facts) = if row.document_kind == "evidence" {
        let entities = entities_for_evidence(connection, &row.evidence_id)?;
        let graph_facts = facts_by_evidence
            .get(&row.evidence_id)
            .cloned()
            .unwrap_or_default();
        (
            evidence_span(connection, &row.evidence_id, graph_version)?,
            entities,
            graph_facts,
        )
    } else {
        (None, Vec::new(), Vec::new())
    };
    let code_artifact = code_artifact_for_document(
        &row.document_kind,
        &row.evidence_id,
        row.source_path.as_deref(),
    );
    let entity_labels = if entities.is_empty() {
        row.entity_labels
    } else {
        entities
            .iter()
            .map(|entity| entity.label.clone())
            .collect::<Vec<_>>()
    };

    Ok(ScoredHit {
        key: if row.document_kind == "evidence" {
            evidence_group_key(
                row.parent_evidence_id
                    .as_deref()
                    .unwrap_or(&row.evidence_id),
            )
        } else {
            row.document_id
        },
        hit: RetrievalHit {
            evidence_id: row.parent_evidence_id.unwrap_or(row.evidence_id),
            source_scope: row.source_scope,
            source_path: row.source_path,
            source_span,
            content: row.content,
            entity_labels,
            entities,
            graph_facts,
            code_artifact,
            retriever_sources: Vec::new(),
            ranking: Vec::new(),
            rerank: None,
            score: 0.0,
        },
        source,
        source_score: -row.rank,
        modality: row.modality,
        explanation: None,
    })
}

#[cfg(test)]
#[path = "bm25_hit_tests.rs"]
mod bm25_hit_tests;
