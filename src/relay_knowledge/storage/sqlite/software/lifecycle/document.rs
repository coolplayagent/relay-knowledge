//! Indexed repository document loading for lifecycle projections.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::storage::StorageError;

pub(super) struct IndexedDocument {
    pub(super) repository_id: String,
    pub(super) source_scope: String,
    pub(super) path: String,
    pub(super) language_id: String,
    pub(super) lines: Vec<IndexedLine>,
}

pub(super) struct IndexedLine {
    pub(super) number: u32,
    pub(super) text: String,
}

pub(super) fn load(
    connection: &Connection,
    source_scope: &str,
) -> Result<Vec<IndexedDocument>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT repository_id, source_scope, path, language_id, content, line_start
        FROM code_repository_chunks
        WHERE source_scope = ?1
        ORDER BY path ASC, line_start ASC, chunk_id ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, u32>(5)?,
        ))
    })?;
    let mut documents = BTreeMap::<String, IndexedDocument>::new();
    for row in rows {
        let (repository_id, source_scope, path, language_id, content, line_start) = row?;
        let document = documents
            .entry(path.clone())
            .or_insert_with(|| IndexedDocument {
                repository_id,
                source_scope,
                path,
                language_id,
                lines: Vec::new(),
            });
        for (offset, text) in content.lines().enumerate() {
            document.lines.push(IndexedLine {
                number: line_start.saturating_add(offset as u32),
                text: text.to_owned(),
            });
        }
    }

    Ok(documents.into_values().collect())
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
