use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::Connection;

mod mutation;
mod version;

pub(super) use mutation::commit_batch;

use crate::{
    domain::GraphVersion,
    storage::{GraphInspection, StorageError},
};

use super::{
    code_graph,
    connection_runtime::{self, maintenance::SqliteMaintenanceState},
    table_stats::count_rows,
};

pub(super) fn inspect_graph(
    connection: &mut Connection,
    database_path: Option<&Path>,
    maintenance_state: &Arc<Mutex<SqliteMaintenanceState>>,
) -> Result<GraphInspection, StorageError> {
    Ok(GraphInspection {
        graph_version: current_graph_version(connection)?,
        entity_count: count_rows(connection, "entities")?,
        evidence_count: count_rows(connection, "evidence")?,
        relation_count: count_rows(connection, "graph_relations")?,
        claim_count: count_rows(connection, "graph_claims")?,
        event_count: count_rows(connection, "graph_events")?,
        mutation_count: count_rows(connection, "graph_mutations")?,
        code_file_count: count_rows(connection, "code_files")?,
        code_symbol_count: count_rows(connection, "code_symbols")?,
        code_reference_count: count_rows(connection, "code_references")?,
        code_chunk_count: count_rows(connection, "code_chunks")?,
        code_parse_status_counts: code_graph::parse_status_counts(connection)?,
        sqlite: connection_runtime::maintenance::diagnostics(
            connection,
            database_path,
            maintenance_state,
        )?,
    })
}

pub(super) fn current_graph_version(
    connection: &mut Connection,
) -> Result<GraphVersion, StorageError> {
    current_graph_version_in_transaction(connection)
}

pub(in crate::storage::sqlite) fn current_graph_version_in_transaction(
    connection: &Connection,
) -> Result<GraphVersion, StorageError> {
    let value = connection.query_row(
        "SELECT graph_version FROM graph_state WHERE id = 1",
        [],
        |row| row.get::<_, u64>(0),
    )?;

    Ok(GraphVersion::new(value))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
