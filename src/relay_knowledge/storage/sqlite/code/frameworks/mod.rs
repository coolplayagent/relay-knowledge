//! Framework graph persistence and bounded repository-scope reads.

use rusqlite::{Connection, Transaction, params, params_from_iter, types::Value};

use crate::{
    domain::{
        CodeFrameworkEdgeRecord, CodeFrameworkNodeRecord, FrameworkEdgeKind, FrameworkGraph,
        FrameworkGraphRequest, FrameworkKind, FrameworkNodeKind, RepositoryCodeRange,
    },
    storage::{FrameworkGraphStore, StorageError, StorageFuture},
};

use super::{
    SearchDocumentInserter, SqliteGraphStore, ensure_queryable_code_scope,
    query::hits::required_repository,
};

impl FrameworkGraphStore for SqliteGraphStore {
    fn search_framework_graph(
        &self,
        request: FrameworkGraphRequest,
    ) -> StorageFuture<'_, FrameworkGraph> {
        self.run_read_snapshot(move |connection| search(connection, request))
    }

    fn search_framework_graph_scope(
        &self,
        source_scope: String,
        request: FrameworkGraphRequest,
    ) -> StorageFuture<'_, FrameworkGraph> {
        self.run_read_snapshot(move |connection| {
            ensure_queryable_code_scope(connection, &source_scope)?;
            search_scope(connection, &source_scope, request)
        })
    }
}

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    nodes: &[CodeFrameworkNodeRecord],
    edges: &[CodeFrameworkEdgeRecord],
) -> Result<(), StorageError> {
    let mut node_statement = transaction.prepare(
        "INSERT OR REPLACE INTO code_repository_framework_nodes (
            repository_id, source_scope, node_id, file_id, path, framework, kind, name,
            detail, symbol_snapshot_id, byte_start, byte_end, line_start, line_end
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut edge_statement = transaction.prepare(
        "INSERT OR REPLACE INTO code_repository_framework_edges (
            repository_id, source_scope, edge_id, file_id, path, framework, kind,
            source_node_id, target_node_id, target_hint, resolution_state,
            confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for node in nodes {
        node_statement.execute(params![
            node.repository_id,
            node.source_scope,
            node.node_id,
            node.file_id,
            node.path,
            node.framework.as_str(),
            node.kind.as_str(),
            node.name,
            node.detail,
            node.symbol_snapshot_id,
            node.byte_range.start,
            node.byte_range.end,
            node.line_range.start,
            node.line_range.end,
        ])?;
        search_documents.insert(
            &node.source_scope,
            "framework_node",
            &node.node_id,
            &node.path,
            framework_language(node.framework),
            [
                node.framework.as_str(),
                node.kind.as_str(),
                node.name.as_str(),
                node.detail.as_deref().unwrap_or_default(),
                node.path.as_str(),
            ],
        )?;
    }
    for edge in edges {
        edge_statement.execute(params![
            edge.repository_id,
            edge.source_scope,
            edge.edge_id,
            edge.file_id,
            edge.path,
            edge.framework.as_str(),
            edge.kind.as_str(),
            edge.source_node_id,
            edge.target_node_id,
            edge.target_hint,
            edge.resolution_state,
            edge.confidence_basis_points,
            edge.confidence_tier,
            edge.byte_range.start,
            edge.byte_range.end,
            edge.line_range.start,
            edge.line_range.end,
        ])?;
        search_documents.insert(
            &edge.source_scope,
            "framework_edge",
            &edge.edge_id,
            &edge.path,
            framework_language(edge.framework),
            [
                edge.framework.as_str(),
                edge.kind.as_str(),
                edge.target_hint.as_deref().unwrap_or_default(),
                edge.path.as_str(),
            ],
        )?;
    }
    search_documents.finish()?;
    Ok(())
}

pub(super) fn search(
    connection: &mut Connection,
    request: FrameworkGraphRequest,
) -> Result<FrameworkGraph, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    let source_scope = status.last_indexed_scope_id.as_deref().ok_or_else(|| {
        StorageError::InvalidInput(format!(
            "code repository '{}' does not have an indexed source scope",
            status.alias
        ))
    })?;
    search_scope(connection, source_scope, request)
}

pub(super) fn search_scope(
    connection: &mut Connection,
    source_scope: &str,
    request: FrameworkGraphRequest,
) -> Result<FrameworkGraph, StorageError> {
    let (node_sql, node_params) = node_query(source_scope, &request);
    let mut node_statement = connection.prepare(&node_sql)?;
    let node_rows = node_statement.query_map(params_from_iter(node_params), |row| {
        Ok(CodeFrameworkNodeRecord {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            node_id: row.get(2)?,
            file_id: row.get(3)?,
            path: row.get(4)?,
            framework: parse_framework(row.get::<_, String>(5)?.as_str())?,
            kind: parse_node_kind(row.get::<_, String>(6)?.as_str())?,
            name: row.get(7)?,
            detail: row.get(8)?,
            symbol_snapshot_id: row.get(9)?,
            byte_range: RepositoryCodeRange {
                start: row.get(10)?,
                end: row.get(11)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(12)?,
                end: row.get(13)?,
            },
        })
    })?;
    let mut nodes = node_rows.collect::<Result<Vec<_>, _>>()?;
    let mut truncated = nodes.len() > request.limit;
    nodes.truncate(request.limit);
    drop(node_statement);

    let (edge_sql, edge_params) = edge_query(source_scope, &request);
    let mut edge_statement = connection.prepare(&edge_sql)?;
    let edge_rows = edge_statement.query_map(params_from_iter(edge_params), |row| {
        Ok(CodeFrameworkEdgeRecord {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            edge_id: row.get(2)?,
            file_id: row.get(3)?,
            path: row.get(4)?,
            framework: parse_framework(row.get::<_, String>(5)?.as_str())?,
            kind: parse_edge_kind(row.get::<_, String>(6)?.as_str())?,
            source_node_id: row.get(7)?,
            target_node_id: row.get(8)?,
            target_hint: row.get(9)?,
            resolution_state: row.get(10)?,
            confidence_basis_points: row.get(11)?,
            confidence_tier: row.get(12)?,
            byte_range: RepositoryCodeRange {
                start: row.get(13)?,
                end: row.get(14)?,
            },
            line_range: RepositoryCodeRange {
                start: row.get(15)?,
                end: row.get(16)?,
            },
        })
    })?;
    let mut edges = edge_rows.collect::<Result<Vec<_>, _>>()?;
    truncated |= edges.len() > request.limit;
    edges.truncate(request.limit);
    drop(edge_statement);
    resolve_edge_targets(connection, source_scope, &mut edges)?;

    Ok(FrameworkGraph {
        nodes,
        edges,
        truncated,
    })
}

fn node_query(source_scope: &str, request: &FrameworkGraphRequest) -> (String, Vec<Value>) {
    let mut sql = String::from(
        "SELECT repository_id, source_scope, node_id, file_id, path, framework, kind, name,
                detail, symbol_snapshot_id, byte_start, byte_end, line_start, line_end
         FROM code_repository_framework_nodes WHERE source_scope = ?",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    push_framework_filter(&mut sql, &mut values, request);
    if !request.kinds.is_empty() {
        push_in_filter(
            &mut sql,
            &mut values,
            "kind",
            request.kinds.iter().map(|kind| kind.as_str()),
        );
    }
    push_path_filter(&mut sql, &mut values, request);
    for term in query_terms(request) {
        sql.push_str(
            " AND (lower(name) LIKE ? ESCAPE '\\' OR lower(COALESCE(detail, '')) LIKE ? ESCAPE '\\' OR lower(path) LIKE ? ESCAPE '\\')",
        );
        values.extend(std::iter::repeat_n(Value::Text(term), 3));
    }
    sql.push_str(" ORDER BY path, line_start, kind, name LIMIT ?");
    values.push(Value::Integer(limit_probe(request.limit)));
    (sql, values)
}

fn edge_query(source_scope: &str, request: &FrameworkGraphRequest) -> (String, Vec<Value>) {
    let mut sql = String::from(
        "SELECT repository_id, source_scope, edge_id, file_id, path, framework, kind,
                source_node_id, target_node_id, target_hint, resolution_state,
                confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end
         FROM code_repository_framework_edges WHERE source_scope = ?",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    push_framework_filter(&mut sql, &mut values, request);
    push_path_filter(&mut sql, &mut values, request);
    for term in query_terms(request) {
        sql.push_str(
            " AND (lower(kind) LIKE ? ESCAPE '\\' OR lower(COALESCE(target_hint, '')) LIKE ? ESCAPE '\\' OR lower(path) LIKE ? ESCAPE '\\')",
        );
        values.extend(std::iter::repeat_n(Value::Text(term), 3));
    }
    sql.push_str(" ORDER BY path, line_start, kind, edge_id LIMIT ?");
    values.push(Value::Integer(limit_probe(request.limit)));
    (sql, values)
}

fn push_framework_filter(
    sql: &mut String,
    values: &mut Vec<Value>,
    request: &FrameworkGraphRequest,
) {
    if !request.frameworks.is_empty() {
        push_in_filter(
            sql,
            values,
            "framework",
            request
                .frameworks
                .iter()
                .map(|framework| framework.as_str()),
        );
    }
}

fn push_in_filter<'a>(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    selected: impl IntoIterator<Item = &'a str>,
) {
    let selected = selected.into_iter().collect::<Vec<_>>();
    sql.push_str(&format!(
        " AND {column} IN ({})",
        std::iter::repeat_n("?", selected.len())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    values.extend(
        selected
            .into_iter()
            .map(|value| Value::Text(value.to_owned())),
    );
}

fn push_path_filter(sql: &mut String, values: &mut Vec<Value>, request: &FrameworkGraphRequest) {
    if request.repository.path_filters.is_empty() {
        return;
    }
    sql.push_str(" AND (");
    for (index, path) in request.repository.path_filters.iter().enumerate() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        sql.push_str("path = ? OR path LIKE ? ESCAPE '\\'");
        values.push(Value::Text(path.clone()));
        values.push(Value::Text(format!("{}/%", escape_like(path))));
    }
    sql.push(')');
}

fn query_terms(request: &FrameworkGraphRequest) -> Vec<String> {
    request
        .query
        .as_deref()
        .into_iter()
        .flat_map(str::split_whitespace)
        .map(|term| format!("%{}%", escape_like(&term.to_ascii_lowercase())))
        .collect()
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn limit_probe(limit: usize) -> i64 {
    i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX)
}

fn resolve_edge_targets(
    connection: &Connection,
    source_scope: &str,
    edges: &mut [CodeFrameworkEdgeRecord],
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "SELECT node_id
         FROM code_repository_framework_nodes
         WHERE source_scope = ?1 AND framework = ?2
           AND (
             path = ?3 OR name = ?3 COLLATE NOCASE OR detail = ?3 COLLATE NOCASE
             OR lower(replace(name, '-', '')) = ?4
             OR lower(replace(COALESCE(detail, ''), '-', '')) = ?4
           )
           AND (
             (?5 = 'owns_template' AND kind = 'template')
             OR (?5 IN ('renders', 'imports') AND kind IN ('component', 'directive', 'pipe'))
             OR (?5 = 'binds_input' AND kind IN ('input', 'prop', 'model'))
             OR (?5 = 'handles_output' AND kind IN ('output', 'emit'))
             OR (?5 = 'writes' AND kind IN ('input', 'output', 'prop', 'emit', 'model'))
             OR (?5 = 'provides_slot' AND kind = 'slot')
           )
         ORDER BY path, line_start, node_id
         LIMIT 2",
    )?;
    for edge in edges
        .iter_mut()
        .filter(|edge| edge.target_node_id.is_none())
    {
        let Some(target_hint) = edge.target_hint.as_deref() else {
            continue;
        };
        let lookup = normalized_target_hint(edge.kind, target_hint);
        let normalized = lookup
            .chars()
            .filter(|character| *character != '-')
            .flat_map(char::to_lowercase)
            .collect::<String>();
        let candidates = statement
            .query_map(
                params![
                    source_scope,
                    edge.framework.as_str(),
                    lookup,
                    normalized,
                    edge.kind.as_str()
                ],
                |row| {
                    Ok(TargetCandidate {
                        node_id: row.get(0)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Vec<_>>();
        if let [candidate] = candidates.as_slice() {
            edge.target_node_id = Some(candidate.node_id.clone());
            edge.resolution_state = "resolved".to_owned();
            edge.confidence_basis_points = edge.confidence_basis_points.max(9_000);
            edge.confidence_tier = "linked".to_owned();
        } else if candidates.len() > 1 {
            edge.resolution_state = "ambiguous".to_owned();
        }
    }
    Ok(())
}

struct TargetCandidate {
    node_id: String,
}

fn normalized_target_hint(kind: FrameworkEdgeKind, hint: &str) -> &str {
    match kind {
        FrameworkEdgeKind::BindsInput
        | FrameworkEdgeKind::HandlesOutput
        | FrameworkEdgeKind::Writes => hint
            .trim_start_matches("v-bind:")
            .trim_start_matches("v-on:")
            .trim_start_matches("v-model:")
            .trim_matches(|character| matches!(character, '[' | ']' | '(' | ')' | '@' | ':')),
        _ => hint,
    }
}

fn framework_language(framework: FrameworkKind) -> &'static str {
    match framework {
        FrameworkKind::Angular => "html",
        FrameworkKind::Vue => "vue",
    }
}

fn parse_framework(value: &str) -> Result<FrameworkKind, rusqlite::Error> {
    match value {
        "angular" => Ok(FrameworkKind::Angular),
        "vue" => Ok(FrameworkKind::Vue),
        _ => Err(invalid_framework_enum("framework")),
    }
}

fn parse_node_kind(value: &str) -> Result<FrameworkNodeKind, rusqlite::Error> {
    match value {
        "component" => Ok(FrameworkNodeKind::Component),
        "directive" => Ok(FrameworkNodeKind::Directive),
        "pipe" => Ok(FrameworkNodeKind::Pipe),
        "template" => Ok(FrameworkNodeKind::Template),
        "input" => Ok(FrameworkNodeKind::Input),
        "output" => Ok(FrameworkNodeKind::Output),
        "prop" => Ok(FrameworkNodeKind::Prop),
        "emit" => Ok(FrameworkNodeKind::Emit),
        "model" => Ok(FrameworkNodeKind::Model),
        "slot" => Ok(FrameworkNodeKind::Slot),
        "template_variable" => Ok(FrameworkNodeKind::TemplateVariable),
        "control_flow" => Ok(FrameworkNodeKind::ControlFlow),
        _ => Err(invalid_framework_enum("framework node kind")),
    }
}

fn parse_edge_kind(value: &str) -> Result<FrameworkEdgeKind, rusqlite::Error> {
    match value {
        "owns_template" => Ok(FrameworkEdgeKind::OwnsTemplate),
        "declares" => Ok(FrameworkEdgeKind::Declares),
        "imports" => Ok(FrameworkEdgeKind::Imports),
        "renders" => Ok(FrameworkEdgeKind::Renders),
        "binds_input" => Ok(FrameworkEdgeKind::BindsInput),
        "handles_output" => Ok(FrameworkEdgeKind::HandlesOutput),
        "reads" => Ok(FrameworkEdgeKind::Reads),
        "writes" => Ok(FrameworkEdgeKind::Writes),
        "uses_directive" => Ok(FrameworkEdgeKind::UsesDirective),
        "provides_slot" => Ok(FrameworkEdgeKind::ProvidesSlot),
        _ => Err(invalid_framework_enum("framework edge kind")),
    }
}

fn invalid_framework_enum(field: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unknown {field} in framework graph storage"),
        )),
    )
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
