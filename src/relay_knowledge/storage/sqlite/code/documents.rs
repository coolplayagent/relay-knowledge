use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::{domain::IndexedRepositoryDocument, storage::StorageError};

use super::code_query_scope::path_filter_allows;

const MAX_DOCUMENT_FILES: usize = 2_048;
const MAX_DOCUMENT_BYTES: usize = 8 * 1_024 * 1_024;

pub(super) fn read_indexed_markdown(
    connection: &mut Connection,
    source_scope: &str,
    path_filters: &[String],
    max_files: usize,
    max_bytes: usize,
) -> Result<Vec<IndexedRepositoryDocument>, StorageError> {
    if max_files == 0 || max_files > MAX_DOCUMENT_FILES {
        return Err(StorageError::InvalidInput(format!(
            "repository document file limit must be between 1 and {MAX_DOCUMENT_FILES}"
        )));
    }
    if max_bytes == 0 || max_bytes > MAX_DOCUMENT_BYTES {
        return Err(StorageError::InvalidInput(format!(
            "repository document byte limit must be between 1 and {MAX_DOCUMENT_BYTES}"
        )));
    }
    let mut statement = connection.prepare(
        "
        SELECT file.path, file.language_id, chunk.content
        FROM code_repository_files file
        JOIN code_repository_chunks chunk
          ON chunk.source_scope = file.source_scope AND chunk.path = file.path
        WHERE file.source_scope = ?1 AND file.language_id = 'markdown'
        ORDER BY file.path ASC, chunk.byte_start ASC, chunk.chunk_id ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut documents = BTreeMap::<String, (String, String)>::new();
    let mut retained_bytes = 0usize;
    for row in rows {
        let (path, language_id, content) = row?;
        if !path_filter_allows(&path, path_filters) {
            continue;
        }
        if !documents.contains_key(&path) && documents.len() >= max_files {
            return Err(StorageError::InvalidInput(
                "repository document file budget exhausted".to_owned(),
            ));
        }
        retained_bytes = retained_bytes.saturating_add(content.len());
        if retained_bytes > max_bytes {
            return Err(StorageError::InvalidInput(
                "repository document byte budget exhausted".to_owned(),
            ));
        }
        let (_, document) = documents
            .entry(path)
            .or_insert_with(|| (language_id, String::new()));
        if !document.is_empty() {
            document.push('\n');
        }
        document.push_str(&content);
    }

    Ok(documents
        .into_iter()
        .map(|(path, (language_id, content))| IndexedRepositoryDocument {
            path,
            language_id,
            content,
        })
        .collect())
}
