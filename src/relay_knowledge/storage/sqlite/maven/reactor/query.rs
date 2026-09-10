//! Reads a bounded module graph without reparsing POMs on the query path.

use super::{MAX_EDGES, MAX_MODULES, persistence::decode};
use crate::storage::sqlite::scope_filters::path_filter_allows;
use crate::{
    domain::{SoftwareBuildTarget, SoftwareGlobalRequest, SoftwareRelationship},
    storage::StorageError,
};
use rusqlite::{Connection, params};

pub(in crate::storage::sqlite) fn projection(
    connection: &Connection,
    scope: &str,
    request: &SoftwareGlobalRequest,
) -> Result<(Vec<SoftwareBuildTarget>, Vec<SoftwareRelationship>), StorageError> {
    super::persistence::require_complete(connection, scope)?;
    if !request.repository.language_filters.is_empty()
        && !request
            .repository
            .language_filters
            .iter()
            .any(|language| matches!(language.as_str(), "java" | "kotlin" | "scala" | "jvm"))
    {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut statement = connection.prepare(
        "SELECT payload FROM maven_reactor_modules WHERE source_scope = ?1 ORDER BY path LIMIT ?2",
    )?;
    let mut modules = Vec::<SoftwareBuildTarget>::new();
    let mut seen = 0;
    for row in statement.query_map(params![scope, MAX_MODULES + 1], |row| {
        row.get::<_, String>(0)
    })? {
        seen += 1;
        if seen > MAX_MODULES {
            return Err(capacity());
        }
        let module: SoftwareBuildTarget = decode(row?)?;
        if path_filter_allows(&module.evidence_path, &request.repository.path_filters) {
            modules.push(module);
            if modules.len() > request.limit {
                return Err(capacity());
            }
        }
    }
    let selected = modules
        .iter()
        .map(|module| module.target_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut statement = connection.prepare("SELECT payload FROM maven_reactor_edges WHERE source_scope = ?1 ORDER BY source_id, kind, edge_id LIMIT ?2")?;
    let mut edges = Vec::new();
    let mut seen = 0;
    for row in statement.query_map(params![scope, MAX_EDGES + 1], |row| row.get::<_, String>(0))? {
        seen += 1;
        if seen > MAX_EDGES {
            return Err(capacity());
        }
        let edge: SoftwareRelationship = decode(row?)?;
        if selected.contains(edge.source_id.as_str())
            && path_filter_allows(&edge.evidence_path, &request.repository.path_filters)
        {
            edges.push(edge);
            if modules.len() + edges.len() > request.limit {
                return Err(capacity());
            }
        }
    }
    Ok((modules, edges))
}

fn capacity() -> StorageError {
    StorageError::CapacityExceeded("Maven module graph exceeds result budget; raise --limit (maximum 500) or narrow the requested path scope; no partial graph was returned".into())
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
