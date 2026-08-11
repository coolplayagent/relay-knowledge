use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    domain::{CommitReceipt, GraphMutationBatch, GraphVersion, SourceScope},
    storage::StorageError,
};

use super::super::{
    evidence_identity::{source_hash_for_evidence, stable_id},
    indexing, retrieval,
};
use super::{current_graph_version_in_transaction, version::storage_version_range};

pub(in crate::storage::sqlite) fn commit_batch(
    connection: &mut Connection,
    batch: GraphMutationBatch,
) -> Result<CommitReceipt, StorageError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    retrieval::ensure_bm25_rebuild_inactive(&transaction)?;
    let current = current_graph_version_in_transaction(&transaction)?;
    let next = GraphVersion::new(current.get() + 1);
    let evidence_count = batch.evidence.len();
    let relation_count = batch.relations.len();
    let claim_count = batch.claims.len();
    let event_count = batch.events.len();
    let mut affected_entity_ids = BTreeSet::new();
    let mut affected_scopes = BTreeSet::new();
    let mut evidence_ids = BTreeSet::new();
    let mut source_hashes = BTreeSet::new();
    let batch_evidence_scopes = batch
        .evidence
        .iter()
        .map(|evidence| {
            (
                evidence.id.clone(),
                evidence.source_scope.as_str().to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for evidence in batch.evidence {
        let evidence_id = evidence.id;
        let source_scope = evidence.source_scope;
        let source_scope_text = source_scope.as_str().to_owned();
        let source_path = evidence.source_path;
        let span = evidence.span;
        let content = evidence.content;
        let entity_labels = evidence.entity_labels;
        let extraction = evidence.extraction;
        let derived_source_hash = source_hash_for_evidence(
            &extraction,
            &source_scope_text,
            source_path.as_deref(),
            &content,
        );
        if let Some(previous_scope) = evidence_scope(&transaction, &evidence_id)? {
            affected_scopes.insert(previous_scope);
        }
        affected_scopes.insert(source_scope_text.clone());
        evidence_ids.insert(evidence_id.clone());
        source_hashes.insert(derived_source_hash.clone());
        if let Some(media_hash) = &extraction.media_hash {
            source_hashes.insert(media_hash.clone());
        }
        if let Some(parent_evidence_id) = extraction.parent_evidence_id.as_deref() {
            validate_parent_evidence(
                &transaction,
                &batch_evidence_scopes,
                &evidence_id,
                &source_scope_text,
                parent_evidence_id,
            )?;
        }
        let layout_region = extraction.layout_region;
        transaction.execute(
            "INSERT INTO evidence (
                 id, source_scope, source_path, span_start_byte, span_end_byte,
                 span_start_line, span_end_line, content, confidence_basis_points,
                 status, modality, source_uri, source_hash, media_hash, extractor,
                 extractor_version, observed_at, parent_evidence_id, layout_page_number,
                 layout_x, layout_y, layout_width, layout_height, embedding_model,
                 embedding_dimension, extraction_status, extraction_message, created_graph_version
             )
             VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26,
                 ?27, ?28
             )
             ON CONFLICT(id) DO UPDATE SET
                 source_scope = excluded.source_scope,
                 source_path = excluded.source_path,
                 span_start_byte = excluded.span_start_byte,
                 span_end_byte = excluded.span_end_byte,
                 span_start_line = excluded.span_start_line,
                 span_end_line = excluded.span_end_line,
                 content = excluded.content,
                 confidence_basis_points = excluded.confidence_basis_points,
                 status = excluded.status,
                 modality = excluded.modality,
                 source_uri = excluded.source_uri,
                 source_hash = excluded.source_hash,
                 media_hash = excluded.media_hash,
                 extractor = excluded.extractor,
                 extractor_version = excluded.extractor_version,
                 observed_at = excluded.observed_at,
                 parent_evidence_id = excluded.parent_evidence_id,
                 layout_page_number = excluded.layout_page_number,
                 layout_x = excluded.layout_x,
                 layout_y = excluded.layout_y,
                 layout_width = excluded.layout_width,
                 layout_height = excluded.layout_height,
                 embedding_model = excluded.embedding_model,
                 embedding_dimension = excluded.embedding_dimension,
                 extraction_status = excluded.extraction_status,
                 extraction_message = excluded.extraction_message,
                 created_graph_version = excluded.created_graph_version",
            params![
                &evidence_id,
                &source_scope_text,
                source_path.as_deref(),
                span.map(|value| value.start_byte),
                span.map(|value| value.end_byte),
                span.map(|value| value.start_line),
                span.map(|value| value.end_line),
                &content,
                evidence.confidence.basis_points,
                evidence.status.as_str(),
                extraction.modality.as_str(),
                extraction.source_uri.as_deref(),
                &derived_source_hash,
                extraction.media_hash.as_deref(),
                extraction.extractor.as_deref(),
                extraction.extractor_version.as_deref(),
                extraction.observed_at.as_deref(),
                extraction.parent_evidence_id.as_deref(),
                layout_region.map(|region| region.page_number),
                layout_region.map(|region| region.x),
                layout_region.map(|region| region.y),
                layout_region.map(|region| region.width),
                layout_region.map(|region| region.height),
                extraction.embedding_model.as_deref(),
                extraction.embedding_dimension.map(i64::from),
                extraction.diagnostic.status.as_str(),
                extraction.diagnostic.message.as_deref(),
                next.get()
            ],
        )?;

        transaction.execute(
            "DELETE FROM evidence_entities WHERE evidence_id = ?1",
            params![&evidence_id],
        )?;

        for label in &entity_labels {
            let entity_id = upsert_entity(&transaction, label, next)?;
            transaction.execute(
                "INSERT OR IGNORE INTO evidence_entities (evidence_id, entity_id)
                 VALUES (?1, ?2)",
                params![evidence_id, entity_id],
            )?;
            affected_entity_ids.insert(entity_id);
        }
        retrieval::replace_evidence_document(
            &transaction,
            retrieval::EvidenceDocumentInput {
                evidence_id: &evidence_id,
                source_scope: &source_scope_text,
                source_path: source_path.as_deref(),
                entity_labels: &entity_labels,
                content: &content,
                status: evidence.status,
                extraction: &extraction,
                source_hash: &derived_source_hash,
                write: retrieval::RetrievalWriteContext {
                    graph_version: next.get(),
                    bm25_target: retrieval::Bm25WriteTarget::Live,
                    refresh_labels: true,
                    refresh_semantic: true,
                    refresh_vector: true,
                },
            },
        )?;
    }

    for relation in batch.relations {
        validate_evidence_references(&transaction, &relation.source_scope, &relation.evidence_ids)?;
        evidence_ids.extend(relation.evidence_ids.iter().cloned());
        let source_entity_id = upsert_entity(&transaction, &relation.source_entity_label, next)?;
        let target_entity_id = upsert_entity(&transaction, &relation.target_entity_label, next)?;
        let version_range = storage_version_range(relation.version_range, next);
        affected_entity_ids.insert(source_entity_id.clone());
        affected_entity_ids.insert(target_entity_id.clone());
        transaction.execute(
            "
            INSERT INTO graph_relations (
                id, source_entity_id, relation_type, target_entity_id,
                evidence_ids_json, confidence_basis_points, status,
                valid_from_graph_version, valid_until_graph_version, created_graph_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                source_entity_id = excluded.source_entity_id,
                relation_type = excluded.relation_type,
                target_entity_id = excluded.target_entity_id,
                evidence_ids_json = excluded.evidence_ids_json,
                confidence_basis_points = excluded.confidence_basis_points,
                status = excluded.status,
                valid_from_graph_version = excluded.valid_from_graph_version,
                valid_until_graph_version = excluded.valid_until_graph_version,
                created_graph_version = excluded.created_graph_version
	        ",
            params![
                relation.id.as_str(),
                source_entity_id,
                relation.relation_type,
                target_entity_id,
                evidence_ids_json(&relation.evidence_ids)?,
                relation.confidence.basis_points,
                relation.status.as_str(),
                version_range.valid_from.get(),
                version_range.valid_until.map(GraphVersion::get),
                next.get(),
            ],
        )?;
        replace_fact_evidence_links(
            &transaction,
            "relation",
            &relation.id,
            &relation.evidence_ids,
        )?;
    }

    for claim in batch.claims {
        validate_evidence_references(&transaction, &claim.source_scope, &claim.evidence_ids)?;
        evidence_ids.extend(claim.evidence_ids.iter().cloned());
        let subject_entity_id = upsert_entity(&transaction, &claim.subject_entity_label, next)?;
        let version_range = storage_version_range(claim.version_range, next);
        affected_entity_ids.insert(subject_entity_id.clone());
        transaction.execute(
            "
            INSERT INTO graph_claims (
                id, subject_entity_id, predicate, object, evidence_ids_json,
                confidence_basis_points, status, valid_from_graph_version,
                valid_until_graph_version, created_graph_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                subject_entity_id = excluded.subject_entity_id,
                predicate = excluded.predicate,
                object = excluded.object,
                evidence_ids_json = excluded.evidence_ids_json,
                confidence_basis_points = excluded.confidence_basis_points,
                status = excluded.status,
                valid_from_graph_version = excluded.valid_from_graph_version,
                valid_until_graph_version = excluded.valid_until_graph_version,
                created_graph_version = excluded.created_graph_version
	        ",
            params![
                claim.id.as_str(),
                subject_entity_id,
                claim.predicate,
                claim.object,
                evidence_ids_json(&claim.evidence_ids)?,
                claim.confidence.basis_points,
                claim.status.as_str(),
                version_range.valid_from.get(),
                version_range.valid_until.map(GraphVersion::get),
                next.get(),
            ],
        )?;
        replace_fact_evidence_links(&transaction, "claim", &claim.id, &claim.evidence_ids)?;
    }

    for event in batch.events {
        validate_evidence_references(&transaction, &event.source_scope, &event.evidence_ids)?;
        evidence_ids.extend(event.evidence_ids.iter().cloned());
        let version_range = storage_version_range(event.version_range, next);
        transaction.execute(
            "
            INSERT INTO graph_events (
                id, event_type, occurred_at, evidence_ids_json,
                confidence_basis_points, status, valid_from_graph_version,
                valid_until_graph_version, created_graph_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                event_type = excluded.event_type,
                occurred_at = excluded.occurred_at,
                evidence_ids_json = excluded.evidence_ids_json,
                confidence_basis_points = excluded.confidence_basis_points,
                status = excluded.status,
                valid_from_graph_version = excluded.valid_from_graph_version,
                valid_until_graph_version = excluded.valid_until_graph_version,
                created_graph_version = excluded.created_graph_version
	        ",
            params![
                event.id.as_str(),
                event.event_type,
                event.occurred_at,
                evidence_ids_json(&event.evidence_ids)?,
                event.confidence.basis_points,
                event.status.as_str(),
                version_range.valid_from.get(),
                version_range.valid_until.map(GraphVersion::get),
                next.get(),
            ],
        )?;
        replace_fact_evidence_links(&transaction, "event", &event.id, &event.evidence_ids)?;
        transaction.execute(
            "DELETE FROM graph_event_entities WHERE event_id = ?1",
            params![event.id],
        )?;
        for label in event.entity_labels {
            let entity_id = upsert_entity(&transaction, &label, next)?;
            affected_entity_ids.insert(entity_id.clone());
            transaction.execute(
                "INSERT OR IGNORE INTO graph_event_entities (event_id, entity_id)
                 VALUES (?1, ?2)",
                params![event.id, entity_id],
            )?;
        }
    }

    transaction.execute(
        "
        DELETE FROM entities
        WHERE id NOT IN (SELECT entity_id FROM evidence_entities)
          AND id NOT IN (SELECT source_entity_id FROM graph_relations)
          AND id NOT IN (SELECT target_entity_id FROM graph_relations)
          AND id NOT IN (SELECT subject_entity_id FROM graph_claims)
          AND id NOT IN (SELECT entity_id FROM graph_event_entities)
        ",
        [],
    )?;

    let entity_count = affected_entity_ids.len();
    add_scopes_for_evidence_ids(&transaction, &evidence_ids, &mut affected_scopes)?;
    if affected_scopes.is_empty()
        && (evidence_count > 0 || relation_count > 0 || claim_count > 0 || event_count > 0)
    {
        affected_scopes.insert(indexing::DEFAULT_SCOPE.to_owned());
    }
    let affected_scopes = affected_scopes.into_iter().collect::<Vec<_>>();
    let affected_entity_ids = affected_entity_ids.into_iter().collect::<Vec<_>>();
    let affected_scopes_json = indexing::json_array(affected_scopes.clone())?;
    let affected_entity_ids_json = indexing::json_array(affected_entity_ids.clone())?;
    let evidence_ids_json = indexing::json_array(evidence_ids)?;
    let source_hashes_json = indexing::json_array(source_hashes)?;
    transaction.execute(
        "INSERT INTO graph_mutations (
             graph_version, evidence_count, entity_count, relation_count, claim_count, event_count,
             affected_scopes_json, affected_entity_ids_json, evidence_ids_json, source_hashes_json
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            next.get(),
            evidence_count,
            entity_count,
            relation_count,
            claim_count,
            event_count,
            affected_scopes_json,
            affected_entity_ids_json,
            evidence_ids_json,
            source_hashes_json
        ],
    )?;
    indexing::mark_mutation_cursors_stale(&transaction, &affected_scopes)?;
    retrieval::mark_bm25_route_graph_version(&transaction, next.get())?;
    transaction.execute(
        "UPDATE graph_state SET graph_version = ?1 WHERE id = 1",
        params![next.get()],
    )?;
    transaction.execute("UPDATE index_status SET state = 'stale'", [])?;
    transaction.commit()?;

    Ok(CommitReceipt {
        graph_version: next,
        evidence_count,
        entity_count,
        relation_count,
        claim_count,
        event_count,
    })
}

fn upsert_entity(
    transaction: &rusqlite::Transaction<'_>,
    label: &str,
    graph_version: GraphVersion,
) -> Result<String, StorageError> {
    let entity_id = stable_id("entity", label);
    transaction.execute(
        "INSERT OR IGNORE INTO entities (id, label, created_graph_version)
         VALUES (?1, ?2, ?3)",
        params![entity_id, label, graph_version.get()],
    )?;

    Ok(entity_id)
}

fn add_scopes_for_evidence_ids(
    connection: &Connection,
    evidence_ids: &BTreeSet<String>,
    affected_scopes: &mut BTreeSet<String>,
) -> Result<(), StorageError> {
    for evidence_id in evidence_ids {
        if let Some(scope) = evidence_scope(connection, evidence_id)? {
            affected_scopes.insert(scope);
        }
    }

    Ok(())
}

fn evidence_scope(
    connection: &Connection,
    evidence_id: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "SELECT source_scope FROM evidence WHERE id = ?1",
            params![evidence_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn validate_parent_evidence(
    connection: &Connection,
    batch_evidence_scopes: &BTreeMap<String, String>,
    evidence_id: &str,
    source_scope: &str,
    parent_evidence_id: &str,
) -> Result<(), StorageError> {
    if parent_evidence_id == evidence_id {
        return Err(StorageError::InvalidInput(
            "parent evidence id must reference a different evidence record".to_owned(),
        ));
    }
    let parent_scope = if let Some(scope) = batch_evidence_scopes.get(parent_evidence_id) {
        Some(scope.clone())
    } else {
        evidence_scope(connection, parent_evidence_id).map_err(|error| {
            StorageError::InvalidInput(format!(
                "parent evidence id '{parent_evidence_id}' could not be validated: {error}"
            ))
        })?
    };

    match parent_scope {
        Some(parent_scope) if parent_scope == source_scope => Ok(()),
        Some(parent_scope) => Err(StorageError::InvalidInput(format!(
            "parent evidence id '{parent_evidence_id}' belongs to source scope \
             '{parent_scope}' instead of '{source_scope}'"
        ))),
        None => Err(StorageError::InvalidInput(format!(
            "parent evidence id '{parent_evidence_id}' does not exist in source scope \
             '{source_scope}'"
        ))),
    }
}

fn evidence_ids_json(evidence_ids: &[String]) -> Result<String, StorageError> {
    serde_json::to_string(evidence_ids)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn validate_evidence_references(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &SourceScope,
    evidence_ids: &[String],
) -> Result<(), StorageError> {
    for evidence_id in evidence_ids {
        let actual_scope = transaction
            .query_row(
                "SELECT source_scope FROM evidence WHERE id = ?1",
                params![evidence_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(actual_scope) = actual_scope else {
            return Err(StorageError::InvalidInput(format!(
                "structured fact references unknown evidence id '{evidence_id}'"
            )));
        };
        if actual_scope != source_scope.as_str() {
            return Err(StorageError::InvalidInput(format!(
                "structured fact references evidence id '{evidence_id}' from source scope \
                 '{actual_scope}' instead of '{}'",
                source_scope.as_str()
            )));
        }
    }

    Ok(())
}

fn replace_fact_evidence_links(
    transaction: &rusqlite::Transaction<'_>,
    fact_kind: &'static str,
    fact_id: &str,
    evidence_ids: &[String],
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM graph_fact_evidence WHERE fact_kind = ?1 AND fact_id = ?2",
        params![fact_kind, fact_id],
    )?;
    for evidence_id in evidence_ids {
        transaction.execute(
            "INSERT OR IGNORE INTO graph_fact_evidence (fact_kind, fact_id, evidence_id)
             VALUES (?1, ?2, ?3)",
            params![fact_kind, fact_id, evidence_id],
        )?;
    }

    Ok(())
}

#[cfg(test)]
#[path = "mutation_tests.rs"]
mod tests;
