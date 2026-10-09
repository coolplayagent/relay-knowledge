//! Exact, scope-constrained evidence documents; never an unbounded graph scan.
use crate::{
    domain::{FactStatus, GraphVersion, SourceScope, StoredEvidenceDocument},
    storage::StorageError,
};
use rusqlite::{Connection, OptionalExtension, params};

pub(in crate::storage::sqlite) fn evidence_document(
    connection: &Connection,
    id: &str,
    source_scope: &str,
) -> Result<Option<StoredEvidenceDocument>, StorageError> {
    if id.is_empty() || id.len() > 1024 {
        return Err(StorageError::InvalidInput(
            "evidence id must contain 1..1024 bytes".into(),
        ));
    }
    SourceScope::parse(source_scope)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let row = connection.query_row(
        "SELECT CASE WHEN length(CAST(content AS BLOB)) <= 2097152 THEN content ELSE NULL END, status, created_graph_version FROM evidence WHERE id = ?1 AND source_scope = ?2",
        params![id, source_scope],
        |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?, row.get::<_, u64>(2)?)),
    ).optional()?;
    row.map(|(content, status, version)| {
        Ok(StoredEvidenceDocument {
            content: content.ok_or_else(|| {
                StorageError::InvalidInput("evidence document exceeds 2 MiB read budget".into())
            })?,
            status: FactStatus::parse(&status)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
            graph_version: GraphVersion::new(version),
        })
    })
    .transpose()
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod tests;
