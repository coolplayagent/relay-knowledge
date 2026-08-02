use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{
    domain::GraphVersion,
    storage::{GraphCanvasStorageEdge, GraphCanvasStorageNode, StorageError},
};

use super::{
    context::{CanvasBuilder, CanvasFilter, collect_rows},
    nodes::{
        detail_map, entity_node, entity_node_id, evidence_node_id, scope_node_id, truncate_label,
    },
};

pub(super) fn add_knowledge_nodes(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    add_evidence(connection, builder, filter)?;
    add_entities(connection, builder, filter)?;
    add_evidence_entity_edges(connection, builder, filter)?;

    Ok(())
}

fn add_evidence(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT id, source_scope, source_path, content, confidence_basis_points,
               status, modality, created_graph_version
        FROM evidence
        WHERE created_graph_version <= ?1
          AND (?2 IS NULL OR source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(id || ' ' || source_scope || ' ' ||
              COALESCE(source_path, '') || ' ' || content) LIKE '%' || lower(?3) || '%'
          )
        ORDER BY created_graph_version DESC, id ASC
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
                row.get::<_, String>(6)?,
                GraphVersion::new(row.get::<_, u64>(7)?),
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    drop(statement);
    builder.observe_query_len(records.len());
    for (id, source_scope, source_path, content, confidence, status, modality, graph_version) in
        records.into_iter().take(filter.limit)
    {
        let mut details = detail_map([
            ("id", id.as_str()),
            ("source_scope", source_scope.as_str()),
            ("content", content.as_str()),
            ("confidence", &confidence.to_string()),
            ("modality", modality.as_str()),
        ]);
        if let Some(path) = source_path.as_deref() {
            details.insert("source_path".to_owned(), path.to_owned());
        }
        builder.insert_node(GraphCanvasStorageNode {
            id: evidence_node_id(&id),
            kind: "evidence".to_owned(),
            label: source_path.clone().unwrap_or_else(|| id.clone()),
            subtitle: Some(truncate_label(&content, 86)),
            source_scope: Some(source_scope.clone()),
            graph_version,
            weight: 2,
            status: Some(status),
            details,
        });
        builder.insert_scope_node(&source_scope, graph_version);
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("scope-evidence:{source_scope}:{id}"),
            kind: "source_scope".to_owned(),
            source: scope_node_id(&source_scope),
            target: evidence_node_id(&id),
            label: "evidence".to_owned(),
            graph_version,
            confidence_basis_points: None,
            evidence_count: None,
            details: BTreeMap::new(),
        });
    }

    Ok(())
}

fn add_entities(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ent.id, ent.label, ent.created_graph_version, MIN(ev.source_scope)
        FROM entities ent
        LEFT JOIN evidence_entities ee ON ee.entity_id = ent.id
        LEFT JOIN evidence ev ON ev.id = ee.evidence_id
                             AND ev.created_graph_version <= ?1
        WHERE ent.created_graph_version <= ?1
          AND (?2 IS NULL OR ev.source_scope = ?2)
          AND (?3 IS NULL OR lower(ent.id || ' ' || ent.label) LIKE '%' || lower(?3) || '%')
        GROUP BY ent.id, ent.label, ent.created_graph_version
        ORDER BY ent.created_graph_version DESC, ent.label ASC
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
                GraphVersion::new(row.get::<_, u64>(2)?),
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    drop(statement);
    builder.observe_query_len(records.len());
    for (id, label, graph_version, source_scope) in records.into_iter().take(filter.limit) {
        builder.insert_node(entity_node(&id, &label, graph_version, source_scope));
    }

    Ok(())
}

fn add_evidence_entity_edges(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT ee.evidence_id, ee.entity_id, ev.created_graph_version
        FROM evidence_entities ee
        JOIN evidence ev ON ev.id = ee.evidence_id
        JOIN entities ent ON ent.id = ee.entity_id
        WHERE ev.created_graph_version <= ?1
          AND (?2 IS NULL OR ev.source_scope = ?2)
          AND (
              ?3 IS NULL OR lower(ent.label || ' ' || ev.content || ' ' ||
              COALESCE(ev.source_path, '')) LIKE '%' || lower(?3) || '%'
          )
        ORDER BY ev.created_graph_version DESC, ee.evidence_id ASC, ee.entity_id ASC
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
                GraphVersion::new(row.get::<_, u64>(2)?),
            ))
        },
    )?;
    let records = collect_rows(rows)?;
    builder.observe_query_len(records.len());
    for (evidence_id, entity_id, graph_version) in records.into_iter().take(filter.limit) {
        builder.insert_edge(GraphCanvasStorageEdge {
            id: format!("evidence-entity:{evidence_id}:{entity_id}"),
            kind: "evidence_link".to_owned(),
            source: evidence_node_id(&evidence_id),
            target: entity_node_id(&entity_id),
            label: "mentions".to_owned(),
            graph_version,
            confidence_basis_points: None,
            evidence_count: Some(1),
            details: BTreeMap::new(),
        });
    }

    Ok(())
}

#[cfg(test)]
#[path = "knowledge_tests.rs"]
mod tests;
