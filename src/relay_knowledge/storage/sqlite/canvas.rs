use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use crate::{
    domain::GraphVersion,
    storage::{
        GraphCanvasSelection, GraphCanvasStorageEdge, GraphCanvasStorageNode,
        GraphCanvasStorageRequest, GraphCanvasStorageSnapshot, StorageError,
    },
};

const MAX_CANVAS_LIMIT: usize = 1000;

pub(super) fn graph_canvas(
    connection: &mut Connection,
    request: GraphCanvasStorageRequest,
) -> Result<GraphCanvasStorageSnapshot, StorageError> {
    validate_limit(request.limit)?;
    let mut builder = CanvasBuilder::new(request.limit);
    let filter = CanvasFilter::new(
        request.source_scope,
        request.query,
        request.graph_version,
        request.limit,
    );

    if request.selection.includes_knowledge() {
        add_knowledge_nodes(connection, &mut builder, &filter)?;
    }
    if request.selection.includes_code() {
        super::canvas_code::add_code_nodes(connection, &mut builder, &filter)?;
    }
    if request.selection == GraphCanvasSelection::Mixed {
        super::canvas_code::add_source_path_links(connection, &mut builder, &filter)?;
    }

    Ok(builder.into_snapshot())
}

fn validate_limit(limit: usize) -> Result<(), StorageError> {
    if limit == 0 {
        return Err(StorageError::InvalidInput(
            "graph canvas limit must be positive".to_owned(),
        ));
    }
    if limit > MAX_CANVAS_LIMIT {
        return Err(StorageError::InvalidInput(format!(
            "graph canvas limit must be at most {MAX_CANVAS_LIMIT}"
        )));
    }

    Ok(())
}

fn add_knowledge_nodes(
    connection: &mut Connection,
    builder: &mut CanvasBuilder,
    filter: &CanvasFilter,
) -> Result<(), StorageError> {
    add_evidence(connection, builder, filter)?;
    add_entities(connection, builder, filter)?;
    add_evidence_entity_edges(connection, builder, filter)?;
    add_relations(connection, builder, filter)?;
    add_claims(connection, builder, filter)?;
    add_events(connection, builder, filter)?;

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

fn entity_node(
    id: &str,
    label: &str,
    graph_version: GraphVersion,
    source_scope: Option<String>,
) -> GraphCanvasStorageNode {
    GraphCanvasStorageNode {
        id: entity_node_id(id),
        kind: "entity".to_owned(),
        label: label.to_owned(),
        subtitle: source_scope.clone(),
        source_scope,
        graph_version,
        weight: 3,
        status: None,
        details: detail_map([("id", id), ("label", label)]),
    }
}

pub(super) fn detail_map<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<String, String> {
    pairs
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn evidence_ids(json: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(json).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

pub(super) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, StorageError> {
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn truncate_label(value: &str, max_chars: usize) -> String {
    let mut text = value.trim().replace('\n', " ");
    if text.chars().count() > max_chars {
        text = text.chars().take(max_chars.saturating_sub(1)).collect();
        text.push_str("...");
    }
    text
}

fn entity_node_id(id: &str) -> String {
    format!("entity:{id}")
}

pub(super) fn evidence_node_id(id: &str) -> String {
    format!("evidence:{id}")
}

fn claim_node_id(id: &str) -> String {
    format!("claim:{id}")
}

fn event_node_id(id: &str) -> String {
    format!("event:{id}")
}

pub(super) fn scope_node_id(scope: &str) -> String {
    format!("scope:{scope}")
}

pub(super) fn code_file_node_id(scope: &str, path: &str) -> String {
    format!("code-file:{scope}:{path}")
}

pub(super) fn code_symbol_node_id(scope: &str, path: &str, symbol_id: &str) -> String {
    format!("code-symbol:{scope}:{path}:{symbol_id}")
}

pub(super) struct CanvasFilter {
    pub(super) source_scope: Option<String>,
    pub(super) query: Option<String>,
    pub(super) graph_version: GraphVersion,
    pub(super) limit: usize,
}

impl CanvasFilter {
    fn new(
        source_scope: Option<String>,
        query: Option<String>,
        graph_version: GraphVersion,
        limit: usize,
    ) -> Self {
        Self {
            source_scope: normalized_filter(source_scope),
            query: normalized_filter(query),
            graph_version,
            limit,
        }
    }

    pub(super) fn sql_limit(&self) -> i64 {
        i64::try_from(self.limit.saturating_add(1)).unwrap_or(i64::MAX)
    }
}

fn normalized_filter(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_owned())
        .filter(|trimmed| !trimmed.is_empty())
}

pub(super) struct CanvasBuilder {
    nodes: BTreeMap<String, GraphCanvasStorageNode>,
    edges: BTreeMap<String, GraphCanvasStorageEdge>,
    available_kinds: BTreeSet<String>,
    limit: usize,
    truncated: bool,
}

impl CanvasBuilder {
    fn new(limit: usize) -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            available_kinds: BTreeSet::new(),
            limit,
            truncated: false,
        }
    }

    pub(super) fn observe_query_len(&mut self, len: usize) {
        if len > self.limit {
            self.truncated = true;
        }
    }

    pub(super) fn insert_scope_node(&mut self, scope: &str, graph_version: GraphVersion) {
        self.insert_node(GraphCanvasStorageNode {
            id: scope_node_id(scope),
            kind: "source_scope".to_owned(),
            label: scope.to_owned(),
            subtitle: Some("source scope".to_owned()),
            source_scope: Some(scope.to_owned()),
            graph_version,
            weight: 3,
            status: None,
            details: detail_map([("source_scope", scope)]),
        });
    }

    pub(super) fn insert_node(&mut self, node: GraphCanvasStorageNode) {
        self.available_kinds.insert(node.kind.clone());
        if self.nodes.contains_key(&node.id) {
            return;
        }
        if self.total_items() >= self.limit {
            self.truncated = true;
            return;
        }
        self.nodes.insert(node.id.clone(), node);
    }

    pub(super) fn insert_edge(&mut self, edge: GraphCanvasStorageEdge) {
        self.available_kinds.insert(edge.kind.clone());
        if self.edges.contains_key(&edge.id) {
            return;
        }
        if !self.nodes.contains_key(&edge.source) || !self.nodes.contains_key(&edge.target) {
            return;
        }
        if self.total_items() >= self.limit {
            self.truncated = true;
            return;
        }
        self.edges.insert(edge.id.clone(), edge);
    }

    fn total_items(&self) -> usize {
        self.nodes.len() + self.edges.len()
    }

    fn into_snapshot(self) -> GraphCanvasStorageSnapshot {
        GraphCanvasStorageSnapshot {
            nodes: self.nodes.into_values().collect(),
            edges: self.edges.into_values().collect(),
            available_kinds: self.available_kinds.into_iter().collect(),
            truncated: self.truncated,
        }
    }
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
