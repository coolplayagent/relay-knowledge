use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{
    domain::GraphVersion,
    storage::{GraphCanvasStorageEdge, GraphCanvasStorageNode, StorageError},
};

use super::{
    context::{CanvasBuilder, CanvasFilter, collect_rows},
    nodes::{
        claim_node_id, detail_map, entity_node, entity_node_id, event_node_id, evidence_node_id,
        truncate_label,
    },
};

pub(super) fn add_structured_facts(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    add_relations(connection, builder, filter)?;
    add_claims(connection, builder, filter)?;
    add_events(connection, builder, filter)?;

    Ok(())
}

fn add_relations(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT rel.id, src.id, src.label, rel.relation_type, tgt.id, tgt.label,
               rel.evidence_ids_json, rel.confidence_basis_points, rel.status,
               rel.created_graph_version, MIN(ev.source_scope)
        FROM graph_relations rel
        JOIN entities src ON src.id = rel.source_entity_id
        JOIN entities tgt ON tgt.id = rel.target_entity_id
        LEFT JOIN graph_fact_evidence gfe ON gfe.fact_kind = 'relation' AND gfe.fact_id = rel.id
        LEFT JOIN evidence ev ON ev.id = gfe.evidence_id
                             AND ev.created_graph_version <= ?1
        WHERE rel.created_graph_version <= ?1
          AND rel.valid_from_graph_version <= ?1
          AND (rel.valid_until_graph_version IS NULL OR rel.valid_until_graph_version >= ?1)
          AND (?2 IS NULL OR ev.source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(rel.id || ' ' || src.label || ' ' ||
              rel.relation_type || ' ' || tgt.label) LIKE '%' || lower(?3) || '%'
          )
        GROUP BY rel.id, src.id, src.label, rel.relation_type, tgt.id, tgt.label,
                 rel.evidence_ids_json, rel.confidence_basis_points, rel.status,
                 rel.created_graph_version
        ORDER BY rel.created_graph_version DESC, rel.id ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.graph_version.get(),
            filter.source_scope.as_deref(),
            filter.query.as_deref(),
            filter.sql_limit()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, u16>(7)?,
                row.get::<_, String>(8)?,
                GraphVersion::new(row.get::<_, u64>(9)?),
                row.get::<_, Option<String>>(10)?,
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    builder.observe_query_len(records.len());
    for (
        id,
        source_id,
        source_label,
        relation_type,
        target_id,
        target_label,
        evidence_json,
        confidence,
        status,
        graph_version,
        source_scope,
    ) in records.into_iter().take(filter.limit)
    {
        let evidence_ids = evidence_ids(&evidence_json)?;
        builder.insert_node(entity_node(
            &source_id,
            &source_label,
            graph_version,
            source_scope.clone(),
        ));
        builder.insert_node(entity_node(
            &target_id,
            &target_label,
            graph_version,
            source_scope,
        ));
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("relation:{id}"),
            kind: "relation".to_owned(),
            source: entity_node_id(&source_id),
            target: entity_node_id(&target_id),
            label: relation_type.clone(),
            graph_version,
            confidence_basis_points: Some(confidence),
            evidence_count: Some(evidence_ids.len()),
            details: detail_map([
                ("id", id.as_str()),
                ("relation_type", relation_type.as_str()),
                ("status", status.as_str()),
                ("evidence_ids", &evidence_ids.join(", ")),
            ]),
        });
    }

    Ok(())
}

fn add_claims(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT claim.id, ent.id, ent.label, claim.predicate, claim.object,
               claim.evidence_ids_json, claim.confidence_basis_points, claim.status,
               claim.created_graph_version, MIN(ev.source_scope)
        FROM graph_claims claim
        JOIN entities ent ON ent.id = claim.subject_entity_id
        LEFT JOIN graph_fact_evidence gfe ON gfe.fact_kind = 'claim' AND gfe.fact_id = claim.id
        LEFT JOIN evidence ev ON ev.id = gfe.evidence_id
                             AND ev.created_graph_version <= ?1
        WHERE claim.created_graph_version <= ?1
          AND claim.valid_from_graph_version <= ?1
          AND (claim.valid_until_graph_version IS NULL OR claim.valid_until_graph_version >= ?1)
          AND (?2 IS NULL OR ev.source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(claim.id || ' ' || ent.label || ' ' ||
              claim.predicate || ' ' || claim.object) LIKE '%' || lower(?3) || '%'
          )
        GROUP BY claim.id, ent.id, ent.label, claim.predicate, claim.object,
                 claim.evidence_ids_json, claim.confidence_basis_points, claim.status,
                 claim.created_graph_version
        ORDER BY claim.created_graph_version DESC, claim.id ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.graph_version.get(),
            filter.source_scope.as_deref(),
            filter.query.as_deref(),
            filter.sql_limit()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, u16>(6)?,
                row.get::<_, String>(7)?,
                GraphVersion::new(row.get::<_, u64>(8)?),
                row.get::<_, Option<String>>(9)?,
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    builder.observe_query_len(records.len());
    for (
        id,
        entity_id,
        entity_label,
        predicate,
        object,
        evidence_json,
        confidence,
        status,
        graph_version,
        source_scope,
    ) in records.into_iter().take(filter.limit)
    {
        let evidence_ids = evidence_ids(&evidence_json)?;
        let label = format!("{predicate}: {object}");
        builder.insert_node(entity_node(
            &entity_id,
            &entity_label,
            graph_version,
            source_scope,
        ));
        builder.insert_node(GraphCanvasStorageNode {
            id: claim_node_id(&id),
            kind: "claim".to_owned(),
            label: truncate_label(&label, 72),
            subtitle: Some(entity_label),
            source_scope: None,
            graph_version,
            weight: 1,
            status: Some(status.clone()),
            details: detail_map([
                ("id", id.as_str()),
                ("predicate", predicate.as_str()),
                ("object", object.as_str()),
                ("status", status.as_str()),
                ("confidence", &confidence.to_string()),
                ("evidence_ids", &evidence_ids.join(", ")),
            ]),
        });
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("claim-subject:{id}:{entity_id}"),
            kind: "claim_subject".to_owned(),
            source: entity_node_id(&entity_id),
            target: claim_node_id(&id),
            label: predicate,
            graph_version,
            confidence_basis_points: Some(confidence),
            evidence_count: Some(evidence_ids.len()),
            details: BTreeMap::new(),
        });
        for evidence_id in evidence_ids {
            builder.insert_edge(GraphCanvasStorageEdge {
                id: format!("evidence-claim:{evidence_id}:{id}"),
                kind: "evidence_link".to_owned(),
                source: evidence_node_id(&evidence_id),
                target: claim_node_id(&id),
                label: "supports".to_owned(),
                graph_version,
                confidence_basis_points: None,
                evidence_count: Some(1),
                details: BTreeMap::new(),
            });
        }
    }

    Ok(())
}

fn add_events(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT event.id, event.event_type, event.occurred_at, event.evidence_ids_json,
               event.confidence_basis_points, event.status, event.created_graph_version,
               MIN(ev.source_scope)
        FROM graph_events event
        LEFT JOIN graph_fact_evidence gfe ON gfe.fact_kind = 'event' AND gfe.fact_id = event.id
        LEFT JOIN evidence ev ON ev.id = gfe.evidence_id
                             AND ev.created_graph_version <= ?1
        WHERE event.created_graph_version <= ?1
          AND event.valid_from_graph_version <= ?1
          AND (event.valid_until_graph_version IS NULL OR event.valid_until_graph_version >= ?1)
          AND (?2 IS NULL OR ev.source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(event.id || ' ' || event.event_type || ' ' ||
              COALESCE(event.occurred_at, '')) LIKE '%' || lower(?3) || '%'
          )
        GROUP BY event.id, event.event_type, event.occurred_at, event.evidence_ids_json,
                 event.confidence_basis_points, event.status, event.created_graph_version
        ORDER BY event.created_graph_version DESC, event.id ASC
        LIMIT ?4
        ",
    )?;
    let rows = statement.query_map(
        params![
            filter.graph_version.get(),
            filter.source_scope.as_deref(),
            filter.query.as_deref(),
            filter.sql_limit()
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u16>(4)?,
                row.get::<_, String>(5)?,
                GraphVersion::new(row.get::<_, u64>(6)?),
                row.get::<_, Option<String>>(7)?,
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    drop(statement);
    builder.observe_query_len(records.len());
    for (id, event_type, occurred_at, evidence_json, confidence, status, graph_version, scope) in
        records.into_iter().take(filter.limit)
    {
        let evidence_ids = evidence_ids(&evidence_json)?;
        let label = occurred_at
            .as_ref()
            .map(|time| format!("{event_type} @ {time}"))
            .unwrap_or_else(|| event_type.clone());
        builder.insert_node(GraphCanvasStorageNode {
            id: event_node_id(&id),
            kind: "event".to_owned(),
            label,
            subtitle: occurred_at.clone(),
            source_scope: scope,
            graph_version,
            weight: 1,
            status: Some(status.clone()),
            details: detail_map([
                ("id", id.as_str()),
                ("event_type", event_type.as_str()),
                ("status", status.as_str()),
                ("confidence", &confidence.to_string()),
                ("evidence_ids", &evidence_ids.join(", ")),
            ]),
        });
        add_event_entity_edges(connection, builder, &id, graph_version)?;
        for evidence_id in evidence_ids {
            builder.insert_edge(GraphCanvasStorageEdge {
                id: format!("evidence-event:{evidence_id}:{id}"),
                kind: "evidence_link".to_owned(),
                source: evidence_node_id(&evidence_id),
                target: event_node_id(&id),
                label: "supports".to_owned(),
                graph_version,
                confidence_basis_points: None,
                evidence_count: Some(1),
                details: BTreeMap::new(),
            });
        }
    }

    Ok(())
}

fn add_event_entity_edges(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    event_id: &str,
    graph_version: GraphVersion,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ent.id, ent.label
        FROM graph_event_entities event_entity
        JOIN entities ent ON ent.id = event_entity.entity_id
        WHERE event_entity.event_id = ?1
        ORDER BY ent.label ASC
        ",
    )?;
    let rows = statement.query_map(params![event_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for record in rows {
        let (entity_id, entity_label) = record?;
        builder.insert_node(entity_node(&entity_id, &entity_label, graph_version, None));
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("event-entity:{event_id}:{entity_id}"),
            kind: "event_entity".to_owned(),
            source: event_node_id(event_id),
            target: entity_node_id(&entity_id),
            label: "involves".to_owned(),
            graph_version,
            confidence_basis_points: None,
            evidence_count: None,
            details: BTreeMap::new(),
        });
    }

    Ok(())
}

fn evidence_ids(json: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(json).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

#[cfg(test)]
#[path = "facts_tests.rs"]
mod tests;
