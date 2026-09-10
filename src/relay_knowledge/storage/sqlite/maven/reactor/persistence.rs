//! Reactor publication shares the existing fenced lifecycle transaction.

use super::{Edge, Module, build};
use crate::{domain::GraphVersion, storage::StorageError};
use rusqlite::{Connection, params};
use serde::{Serialize, de::DeserializeOwned};

pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS maven_reactor_status (
            source_scope TEXT PRIMARY KEY, complete INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS maven_reactor_modules (
            source_scope TEXT NOT NULL, module_id TEXT NOT NULL, path TEXT NOT NULL,
            directory TEXT NOT NULL, payload TEXT NOT NULL,
            PRIMARY KEY(source_scope, module_id), UNIQUE(source_scope, path)
        );
        CREATE TABLE IF NOT EXISTS maven_reactor_edges (
            source_scope TEXT NOT NULL, edge_id TEXT NOT NULL, source_id TEXT NOT NULL,
            target_id TEXT NOT NULL, kind TEXT NOT NULL, resolution_state TEXT NOT NULL,
            dependency_scope TEXT NOT NULL, profile TEXT, payload TEXT NOT NULL,
            PRIMARY KEY(source_scope, edge_id)
        );
        CREATE INDEX IF NOT EXISTS maven_reactor_reverse
            ON maven_reactor_edges(source_scope, target_id, kind, resolution_state);
    ",
    )?;
    Ok(())
}

pub(in crate::storage::sqlite) fn refresh(
    connection: &Connection,
    scope: &str,
    version: GraphVersion,
) -> Result<(), StorageError> {
    let loaded = super::super::effective_models(connection, scope)?;
    let model_paths = loaded
        .models
        .iter()
        .map(|model| model.document.path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut statement = connection.prepare("SELECT path FROM code_repository_files WHERE source_scope = ?1 AND (path = 'pom.xml' OR path LIKE '%/pom.xml') ORDER BY path LIMIT ?2")?;
    let paths = statement
        .query_map(params![scope, super::MAX_MODULES + 1], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if paths.len() > super::MAX_MODULES {
        return Err(StorageError::CapacityExceeded(
            "Maven reactor module budget exceeded".into(),
        ));
    }
    if loaded.preserve_existing_facts
        || paths
            .iter()
            .any(|path| !model_paths.contains(path.as_str()))
    {
        connection.execute(
            "INSERT OR REPLACE INTO maven_reactor_status VALUES (?1, 0)",
            [scope],
        )?;
        return Ok(());
    }
    let (modules, edges) = build::facts(&loaded.models, version)?;
    persist(connection, scope, &modules, &edges)
}

pub(super) fn persist(
    connection: &Connection,
    scope: &str,
    modules: &[Module],
    edges: &[Edge],
) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM maven_reactor_edges WHERE source_scope = ?1",
        [scope],
    )?;
    connection.execute(
        "DELETE FROM maven_reactor_modules WHERE source_scope = ?1",
        [scope],
    )?;
    let mut insert =
        connection.prepare("INSERT INTO maven_reactor_modules VALUES (?1, ?2, ?3, ?4, ?5)")?;
    for module in modules {
        insert.execute(params![
            scope,
            module.target.target_id,
            module.target.evidence_path,
            module.directory,
            encode(&module.target)?
        ])?;
    }
    let mut insert = connection
        .prepare("INSERT INTO maven_reactor_edges VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)")?;
    for edge in edges {
        let fact = &edge.relationship;
        insert.execute(params![
            scope,
            fact.relationship_id,
            fact.source_id,
            fact.target_id,
            fact.relationship_kind,
            fact.resolution_state,
            edge.dependency_scope,
            edge.profile,
            encode(fact)?
        ])?;
    }
    connection.execute(
        "INSERT OR REPLACE INTO maven_reactor_status VALUES (?1, 1)",
        [scope],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite) fn require_complete(
    connection: &Connection,
    scope: &str,
) -> Result<(), StorageError> {
    use rusqlite::OptionalExtension;
    let complete = connection
        .query_row(
            "SELECT complete FROM maven_reactor_status WHERE source_scope = ?1",
            [scope],
            |row| row.get::<_, bool>(0),
        )
        .optional()?;
    let missing_maven_projection = if complete.is_none() {
        connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM code_repository_files WHERE source_scope = ?1 AND (path = 'pom.xml' OR path LIKE '%/pom.xml'))",
            [scope], |row| row.get::<_, bool>(0),
        )?
    } else {
        false
    };
    if complete == Some(false) || missing_maven_projection {
        return Err(StorageError::InvalidInput("Maven module graph is missing or incomplete; repair and reindex the indexed POM evidence".into()));
    }
    Ok(())
}

fn encode(value: &impl Serialize) -> Result<String, StorageError> {
    let json =
        serde_json::to_string(value).map_err(|error| StorageError::Invariant(error.to_string()))?;
    if json.len() > 32_768 {
        return Err(StorageError::CapacityExceeded(
            "Maven reactor fact exceeds 32 KiB".into(),
        ));
    }
    Ok(json)
}

pub(super) fn decode<T: DeserializeOwned>(json: String) -> Result<T, StorageError> {
    serde_json::from_str(&json)
        .map_err(|error| StorageError::Invariant(format!("invalid Maven reactor fact: {error}")))
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
