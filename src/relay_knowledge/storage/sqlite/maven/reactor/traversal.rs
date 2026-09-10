//! Reverse BFS over resolved direct reactor edges, with explicit graph budgets.

use super::{MAX_EDGES, MAX_MODULES, ModuleImpact, persistence::decode};
use crate::storage::sqlite::scope_filters::path_filter_allows;
use crate::{domain::SoftwareRelationship, storage::StorageError};
use rusqlite::{Connection, params};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(in crate::storage::sqlite) fn downstream(
    connection: &Connection,
    scope: &str,
    changed: &BTreeSet<String>,
    filters: &[String],
) -> Result<Vec<ModuleImpact>, StorageError> {
    if changed.is_empty() {
        return Ok(Vec::new());
    }
    super::persistence::require_complete(connection, scope)?;
    let mut statement = connection.prepare("SELECT module_id, directory, path FROM maven_reactor_modules WHERE source_scope = ?1 ORDER BY length(directory) DESC, path LIMIT ?2")?;
    let modules = statement
        .query_map(params![scope, MAX_MODULES + 1], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if modules.len() > MAX_MODULES {
        return Err(budget());
    }
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    for path in changed {
        if let Some((id, _, pom)) = modules.iter().find(|(_, directory, _)| {
            directory.is_empty() || path == directory || path.starts_with(&format!("{directory}/"))
        }) {
            if visited.insert(id.clone()) {
                queue.push_back((id.clone(), vec![pom.clone()]));
            }
        }
    }
    let module_paths = modules
        .iter()
        .map(|(id, _, path)| (id.as_str(), path.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut statement = connection.prepare("SELECT payload FROM maven_reactor_edges WHERE source_scope = ?1 AND target_id = ?2 AND kind = 'depends_on' AND resolution_state = 'resolved' AND dependency_scope IN ('compile', 'runtime', 'provided', 'test', 'system') AND profile IS NULL ORDER BY source_id, edge_id LIMIT ?3")?;
    let mut result = Vec::new();
    let mut scanned = 0usize;
    while let Some((target, chain)) = queue.pop_front() {
        for row in statement.query_map(
            params![scope, target, MAX_EDGES.saturating_sub(scanned) + 1],
            |row| row.get::<_, String>(0),
        )? {
            scanned += 1;
            if scanned > MAX_EDGES {
                return Err(budget());
            }
            let edge: SoftwareRelationship = decode(row?)?;
            if visited.contains(&edge.source_id) {
                continue;
            }
            let Some(source_path) = module_paths.get(edge.source_id.as_str()) else {
                continue;
            };
            if !path_filter_allows(source_path, filters)
                || !path_filter_allows(&edge.evidence_path, filters)
            {
                continue;
            }
            if chain.len() >= 64 || visited.len() >= MAX_MODULES {
                return Err(budget());
            }
            visited.insert(edge.source_id.clone());
            let mut next = vec![(*source_path).to_owned()];
            next.extend(chain.iter().cloned());
            result.push(ModuleImpact {
                path: edge.evidence_path,
                line: edge.evidence_line_range.start,
                chain: next.join(" -> "),
            });
            queue.push_back((edge.source_id, next));
        }
    }
    Ok(result)
}

fn budget() -> StorageError {
    StorageError::CapacityExceeded(
        "Maven impact exceeds 8192 modules, 131072 edges or 64-hop budget; narrow the scope".into(),
    )
}

#[cfg(test)]
#[path = "traversal_tests.rs"]
mod tests;
