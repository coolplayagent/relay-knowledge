//! Candidate-driven repository-set overlay projection.

use std::collections::BTreeMap;

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{
    domain::CodeRepositoryCrossEdge,
    storage::{CodeRepositorySetEdgeSelector, StorageError},
};

const EDGE_SELECTOR_BATCH_SIZE: usize = 128;
const EDGE_COLUMNS: &str = "
    edge.edge_id, edge.set_id, edge.from_source_scope, edge.from_repository_id,
    edge.from_record_kind, edge.from_record_id, edge.to_source_scope,
    edge.to_repository_id, edge.to_record_kind, edge.to_record_id, edge.edge_kind,
    edge.resolution_state, edge.confidence_basis_points, edge.confidence_tier,
    edge.evidence_json, edge.created_at_ms
";

pub(in crate::storage::sqlite::code) fn cross_edges_for_selector(
    connection: &mut Connection,
    set_id: &str,
    selector: &CodeRepositorySetEdgeSelector,
) -> Result<Vec<CodeRepositoryCrossEdge>, StorageError> {
    let mut selected = BTreeMap::new();
    for origins in selector.origin_files.chunks(EDGE_SELECTOR_BATCH_SIZE) {
        select_origin_edges(connection, set_id, origins, &mut selected)?;
    }
    for targets in selector.target_records.chunks(EDGE_SELECTOR_BATCH_SIZE) {
        select_target_edges(connection, set_id, targets, &mut selected)?;
    }
    let mut edges = selected.into_values().collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.from_source_scope
            .cmp(&right.from_source_scope)
            .then_with(|| left.from_record_id.cmp(&right.from_record_id))
            .then_with(|| left.edge_id.cmp(&right.edge_id))
    });
    Ok(edges)
}

fn select_origin_edges(
    connection: &Connection,
    set_id: &str,
    origins: &[(String, String)],
    selected: &mut BTreeMap<String, CodeRepositoryCrossEdge>,
) -> Result<(), StorageError> {
    if origins.is_empty() {
        return Ok(());
    }
    let values_sql = selector_values_sql(origins.len(), 2);
    let sql = format!(
        "
        WITH selected_origin(source_scope, path) AS (VALUES {values_sql})
        SELECT {EDGE_COLUMNS}
        FROM code_repository_cross_edges edge
        INNER JOIN selected_origin selected
            ON selected.source_scope = edge.from_source_scope
           AND selected.path = CASE
               WHEN json_valid(edge.evidence_json)
               THEN json_extract(edge.evidence_json, '$.from_path')
           END
        WHERE edge.set_id = ?
          AND edge.from_record_kind = 'module_reference'
          AND EXISTS (
              SELECT 1
              FROM code_repository_set_members member
              WHERE member.set_id = edge.set_id
                AND member.source_scope = edge.from_source_scope
          )
        "
    );
    let mut values = Vec::with_capacity(origins.len() * 2 + 1);
    for (source_scope, path) in origins {
        values.push(Value::Text(source_scope.clone()));
        values.push(Value::Text(path.clone()));
    }
    values.push(Value::Text(set_id.to_owned()));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), super::edge_from_row)?;
    for edge in rows {
        let edge = edge?;
        selected.insert(edge.edge_id.clone(), edge);
    }
    Ok(())
}

fn select_target_edges(
    connection: &Connection,
    set_id: &str,
    targets: &[(String, String, String)],
    selected: &mut BTreeMap<String, CodeRepositoryCrossEdge>,
) -> Result<(), StorageError> {
    if targets.is_empty() {
        return Ok(());
    }
    let values_sql = selector_values_sql(targets.len(), 3);
    let sql = format!(
        "
        WITH selected_target(source_scope, record_kind, record_id) AS (VALUES {values_sql})
        SELECT {EDGE_COLUMNS}
        FROM code_repository_cross_edges edge
        INNER JOIN selected_target selected
            ON selected.source_scope = edge.to_source_scope
           AND selected.record_kind = edge.to_record_kind
           AND selected.record_id = edge.to_record_id
        WHERE edge.set_id = ?
          AND EXISTS (
              SELECT 1
              FROM code_repository_set_members member
              WHERE member.set_id = edge.set_id
                AND member.source_scope = edge.from_source_scope
          )
        "
    );
    let mut values = Vec::with_capacity(targets.len() * 3 + 1);
    for (source_scope, record_kind, record_id) in targets {
        values.push(Value::Text(source_scope.clone()));
        values.push(Value::Text(record_kind.clone()));
        values.push(Value::Text(record_id.clone()));
    }
    values.push(Value::Text(set_id.to_owned()));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values), super::edge_from_row)?;
    for edge in rows {
        let edge = edge?;
        selected.insert(edge.edge_id.clone(), edge);
    }
    Ok(())
}

fn selector_values_sql(row_count: usize, column_count: usize) -> String {
    let row = format!("({})", vec!["?"; column_count].join(", "));
    vec![row; row_count].join(", ")
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
