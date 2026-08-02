use rusqlite::{Connection, params};

use crate::{
    domain::{ConfidenceScore, ContextGraphFact, ContextGraphFactKind, RetrieverSource},
    storage::{GraphSearchRequest, StorageError},
};

use super::{
    event::{load_events, occurred_label},
    support::SupportContext,
};
use crate::storage::sqlite::retrieval::{
    ScoredHit,
    context::{parse_fact_status, version_range},
    local_model::overlap_score,
    sort_scored_hits,
};

pub(in crate::storage::sqlite::retrieval) fn path_candidates(
    connection: &Connection,
    request: &GraphSearchRequest,
) -> Result<Vec<ScoredHit>, StorageError> {
    let mut hits = Vec::new();
    collect_relation_paths(connection, request, &mut hits)?;
    collect_claim_paths(connection, request, &mut hits)?;
    collect_event_paths(connection, request, &mut hits)?;
    sort_scored_hits(&mut hits);

    Ok(hits)
}

fn collect_relation_paths(
    connection: &Connection,
    request: &GraphSearchRequest,
    hits: &mut Vec<ScoredHit>,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT gr.id, src.label, gr.relation_type, dst.label, gr.evidence_ids_json,
               gr.confidence_basis_points, gr.status, gr.valid_from_graph_version,
               gr.valid_until_graph_version
        FROM graph_relations gr
        INNER JOIN entities src ON src.id = gr.source_entity_id
        INNER JOIN entities dst ON dst.id = gr.target_entity_id
        WHERE gr.status = 'accepted'
          AND gr.created_graph_version <= ?1
          AND gr.valid_from_graph_version <= ?1
          AND (gr.valid_until_graph_version IS NULL OR gr.valid_until_graph_version >= ?1)
        ORDER BY gr.created_graph_version DESC, gr.id ASC
        ",
    )?;
    let rows = statement.query_map(params![request.graph_version.get()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u16>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, u64>(7)?,
            row.get::<_, Option<u64>>(8)?,
        ))
    })?;
    for (id, source, relation_type, target, evidence_ids_json, confidence, status, from, until) in
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?
    {
        let Some(context) = SupportContext::load(connection, &evidence_ids_json, request)? else {
            continue;
        };
        let text = format!("{source} {relation_type} {target} {}", context.content);
        let score = overlap_score(
            &request.query,
            &text,
            &context.entity_labels,
            context.source_path.as_deref(),
        );
        if score > 0.0 {
            let content = format!(
                "{source} -[{relation_type}]-> {target}\n{}",
                context.content
            );
            let graph_fact = ContextGraphFact {
                fact_id: id.clone(),
                kind: ContextGraphFactKind::Relation,
                subject: source,
                predicate: relation_type,
                object: Some(target),
                evidence_ids: context.evidence_ids.clone(),
                confidence: ConfidenceScore {
                    basis_points: confidence,
                },
                status: parse_fact_status(&status)?,
                version_range: version_range(from, until)?,
            };
            hits.push(context.scored(
                content,
                RetrieverSource::GraphPath,
                score,
                format!("relation path {id} supported by scoped evidence"),
                Some(graph_fact),
            ));
        }
    }

    Ok(())
}

fn collect_claim_paths(
    connection: &Connection,
    request: &GraphSearchRequest,
    hits: &mut Vec<ScoredHit>,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT gc.id, ent.label, gc.predicate, gc.object, gc.evidence_ids_json,
               gc.confidence_basis_points, gc.status, gc.valid_from_graph_version,
               gc.valid_until_graph_version
        FROM graph_claims gc
        INNER JOIN entities ent ON ent.id = gc.subject_entity_id
        WHERE gc.status = 'accepted'
          AND gc.created_graph_version <= ?1
          AND gc.valid_from_graph_version <= ?1
          AND (gc.valid_until_graph_version IS NULL OR gc.valid_until_graph_version >= ?1)
        ORDER BY gc.created_graph_version DESC, gc.id ASC
        ",
    )?;
    let rows = statement.query_map(params![request.graph_version.get()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u16>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, u64>(7)?,
            row.get::<_, Option<u64>>(8)?,
        ))
    })?;
    for (id, subject, predicate, object, evidence_ids_json, confidence, status, from, until) in rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
    {
        let Some(context) = SupportContext::load(connection, &evidence_ids_json, request)? else {
            continue;
        };
        let text = format!("{subject} {predicate} {object} {}", context.content);
        let score = overlap_score(
            &request.query,
            &text,
            &context.entity_labels,
            context.source_path.as_deref(),
        );
        if score > 0.0 {
            let content = format!("claim {subject} {predicate} {object}\n{}", context.content);
            let graph_fact = ContextGraphFact {
                fact_id: id.clone(),
                kind: ContextGraphFactKind::Claim,
                subject,
                predicate,
                object: Some(object),
                evidence_ids: context.evidence_ids.clone(),
                confidence: ConfidenceScore {
                    basis_points: confidence,
                },
                status: parse_fact_status(&status)?,
                version_range: version_range(from, until)?,
            };
            hits.push(context.scored(
                content,
                RetrieverSource::GraphPath,
                score,
                format!("schema-guided claim path {id} supported by scoped evidence"),
                Some(graph_fact),
            ));
        }
    }

    Ok(())
}

fn collect_event_paths(
    connection: &Connection,
    request: &GraphSearchRequest,
    hits: &mut Vec<ScoredHit>,
) -> Result<(), StorageError> {
    for event in load_events(connection, request)? {
        let Some(context) = SupportContext::load(connection, &event.evidence_ids_json, request)?
        else {
            continue;
        };
        let text = format!(
            "{} {} {} {}",
            event.event_type,
            event.occurred_at.as_deref().unwrap_or_default(),
            event.labels,
            context.content
        );
        let score = overlap_score(
            &request.query,
            &text,
            &context.entity_labels,
            context.source_path.as_deref(),
        );
        if score > 0.0 {
            let occurred = occurred_label(event.occurred_at.as_deref());
            let content = format!(
                "event {}{}: {}\n{}",
                event.event_type, occurred, event.labels, context.content
            );
            let graph_fact = event.graph_fact(&context)?;
            hits.push(context.scored(
                content,
                RetrieverSource::GraphPath,
                score,
                format!(
                    "schema-guided event path {} supported by scoped evidence",
                    event.id
                ),
                Some(graph_fact),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
