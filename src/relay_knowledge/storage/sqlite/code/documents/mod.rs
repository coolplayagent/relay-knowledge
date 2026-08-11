use std::collections::BTreeMap;

use rusqlite::{Connection, params_from_iter, types::Value};

use crate::{domain::IndexedRepositoryDocument, storage::StorageError};

use super::code_query_scope::{normalize_path_filter, path_matches_filter};

const MAX_DOCUMENT_FILES: usize = 2_048;
const MAX_DOCUMENT_BYTES: usize = 8 * 1_024 * 1_024;
const MAX_DOCUMENT_PATH_FILTERS: usize = 256;
const DOCUMENT_BYTE_BUDGET_EXHAUSTED: &str = "repository document byte budget exhausted";

pub(super) fn read_indexed_markdown(
    connection: &mut Connection,
    source_scope: &str,
    path_filters: &[String],
    max_files: usize,
    max_bytes: usize,
) -> Result<Vec<IndexedRepositoryDocument>, StorageError> {
    validate_budgets(max_files, max_bytes)?;
    let path_predicate = SqlPathPredicate::new(path_filters)?;
    let transaction = connection.transaction()?;
    let expected_files = preflight_snapshot_documents(
        &transaction,
        source_scope,
        &path_predicate,
        max_files,
        max_bytes,
    )?;
    let documents = materialize_snapshot_documents(
        &transaction,
        source_scope,
        &path_predicate,
        expected_files,
        max_bytes,
    )?;
    transaction.commit()?;

    Ok(documents)
}

fn validate_budgets(max_files: usize, max_bytes: usize) -> Result<(), StorageError> {
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

    Ok(())
}

fn preflight_snapshot_documents(
    connection: &Connection,
    source_scope: &str,
    path_predicate: &SqlPathPredicate,
    max_files: usize,
    max_bytes: usize,
) -> Result<usize, StorageError> {
    let (sql, values) =
        bounded_file_metadata_query(source_scope, path_predicate, max_files.saturating_add(1));

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut file_count = 0usize;
    let mut indexed_bytes = 0usize;
    for row in rows {
        let (path, byte_len) = row?;
        if file_count == max_files {
            return Err(StorageError::InvalidInput(
                "repository document file budget exhausted".to_owned(),
            ));
        }
        let byte_len = usize::try_from(byte_len).map_err(|_| {
            StorageError::InvalidInput(format!(
                "repository document '{path}' has invalid indexed byte length"
            ))
        })?;
        indexed_bytes = indexed_bytes
            .checked_add(byte_len)
            .ok_or_else(|| StorageError::InvalidInput(DOCUMENT_BYTE_BUDGET_EXHAUSTED.to_owned()))?;
        if indexed_bytes > max_bytes {
            return Err(StorageError::InvalidInput(
                DOCUMENT_BYTE_BUDGET_EXHAUSTED.to_owned(),
            ));
        }
        file_count += 1;
    }

    Ok(file_count)
}

fn bounded_file_metadata_query(
    source_scope: &str,
    path_predicate: &SqlPathPredicate,
    row_limit: usize,
) -> (String, Vec<Value>) {
    let mut sql = String::from(
        "
        SELECT file.path, file.byte_len
        FROM code_repository_files file
        WHERE file.source_scope = ?
          AND file.language_id = 'markdown'
          AND EXISTS (
              SELECT 1
              FROM code_repository_chunks chunk
              WHERE chunk.source_scope = file.source_scope
                AND chunk.path = file.path
          )
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    path_predicate.append_sql(&mut sql, &mut values, "file.path");
    sql.push_str(" ORDER BY file.path ASC LIMIT ?");
    values.push(Value::Integer(i64::try_from(row_limit).unwrap_or(i64::MAX)));

    (sql, values)
}

fn materialize_snapshot_documents(
    connection: &Connection,
    source_scope: &str,
    path_predicate: &SqlPathPredicate,
    expected_files: usize,
    max_bytes: usize,
) -> Result<Vec<IndexedRepositoryDocument>, StorageError> {
    let (sql, values) = bounded_document_content_query(source_scope, path_predicate);

    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(RawDocumentChunk {
            path: row.get(0)?,
            language_id: row.get(1)?,
            file_byte_len: row.get(2)?,
            content: row.get(3)?,
            byte_start: row.get(4)?,
            byte_end: row.get(5)?,
        })
    })?;
    let mut documents = BTreeMap::<String, DocumentAssembly>::new();
    let mut materialized_bytes = 0usize;
    for row in rows {
        let chunk = row?;
        let path = chunk.path.clone();
        let document = documents.entry(path.clone()).or_insert_with(|| {
            DocumentAssembly::new(chunk.language_id.clone(), chunk.file_byte_len)
        });
        document.append_lossless_chunk(&path, chunk, &mut materialized_bytes, max_bytes)?;
    }
    if documents.len() != expected_files {
        return Err(StorageError::InvalidInput(
            "repository document snapshot changed while it was read".to_owned(),
        ));
    }

    documents
        .into_iter()
        .map(|(path, document)| document.finish(path))
        .collect()
}

fn bounded_document_content_query(
    source_scope: &str,
    path_predicate: &SqlPathPredicate,
) -> (String, Vec<Value>) {
    let mut sql = String::from(
        "
        SELECT file.path, file.language_id, file.byte_len, chunk.content,
               chunk.byte_start, chunk.byte_end
        FROM code_repository_files file
        CROSS JOIN code_repository_chunks chunk
        WHERE file.source_scope = ?
          AND file.language_id = 'markdown'
          AND chunk.source_scope = file.source_scope
          AND chunk.path = file.path
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    path_predicate.append_sql(&mut sql, &mut values, "file.path");
    sql.push_str(" ORDER BY file.path ASC, chunk.byte_start ASC, chunk.chunk_id ASC");

    (sql, values)
}

struct RawDocumentChunk {
    path: String,
    language_id: String,
    file_byte_len: i64,
    content: String,
    byte_start: i64,
    byte_end: i64,
}

struct DocumentAssembly {
    language_id: String,
    content: String,
    file_byte_len: i64,
    next_byte_start: usize,
    seen_chunk: bool,
}

impl DocumentAssembly {
    fn new(language_id: String, file_byte_len: i64) -> Self {
        Self {
            language_id,
            content: String::new(),
            file_byte_len,
            next_byte_start: 0,
            seen_chunk: false,
        }
    }

    fn append_lossless_chunk(
        &mut self,
        path: &str,
        chunk: RawDocumentChunk,
        materialized_bytes: &mut usize,
        max_bytes: usize,
    ) -> Result<(), StorageError> {
        let byte_start = usize::try_from(chunk.byte_start).map_err(|_| lossy_document(path))?;
        let byte_end = usize::try_from(chunk.byte_end).map_err(|_| lossy_document(path))?;
        let is_single_empty_file_chunk = !self.seen_chunk
            && self.file_byte_len == 0
            && byte_start == 0
            && byte_end == 0
            && chunk.content.is_empty();
        if chunk.language_id != self.language_id
            || chunk.file_byte_len != self.file_byte_len
            || byte_start != self.next_byte_start
            || (byte_end <= byte_start && !is_single_empty_file_chunk)
            || byte_end.saturating_sub(byte_start) != chunk.content.len()
        {
            return Err(lossy_document(path));
        }
        *materialized_bytes = materialized_bytes
            .checked_add(chunk.content.len())
            .ok_or_else(|| StorageError::InvalidInput(DOCUMENT_BYTE_BUDGET_EXHAUSTED.to_owned()))?;
        if *materialized_bytes > max_bytes {
            return Err(StorageError::InvalidInput(
                DOCUMENT_BYTE_BUDGET_EXHAUSTED.to_owned(),
            ));
        }
        self.content.push_str(&chunk.content);
        self.next_byte_start = byte_end;
        self.seen_chunk = true;

        Ok(())
    }

    fn finish(self, path: String) -> Result<IndexedRepositoryDocument, StorageError> {
        let file_byte_len =
            usize::try_from(self.file_byte_len).map_err(|_| lossy_document(&path))?;
        if self.next_byte_start != file_byte_len || self.content.len() != file_byte_len {
            return Err(lossy_document(&path));
        }

        Ok(IndexedRepositoryDocument {
            path,
            language_id: self.language_id,
            content: self.content,
        })
    }
}

fn lossy_document(path: &str) -> StorageError {
    StorageError::InvalidInput(format!(
        "repository document '{path}' is not lossless in the indexed snapshot; re-index the repository"
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SqlPathPredicate {
    All,
    None,
    Roots(Vec<String>),
}

impl SqlPathPredicate {
    fn new(path_filters: &[String]) -> Result<Self, StorageError> {
        if path_filters.len() > MAX_DOCUMENT_PATH_FILTERS {
            return Err(StorageError::InvalidInput(format!(
                "repository document path filter limit must not exceed {MAX_DOCUMENT_PATH_FILTERS}"
            )));
        }
        if path_filters.is_empty() {
            return Ok(Self::All);
        }

        let mut roots = Vec::new();
        for filter in path_filters {
            let root = normalize_path_filter(filter);
            if root == "." {
                return Ok(Self::All);
            }
            if root.contains('\0') {
                return Err(StorageError::InvalidInput(
                    "repository document path filters must not contain NUL".to_owned(),
                ));
            }
            if !root.is_empty() && !roots.iter().any(|existing| existing == root) {
                roots.push(root.to_owned());
            }
        }
        roots.sort();
        let mut minimal_roots = Vec::<String>::new();
        for root in roots {
            if !minimal_roots
                .iter()
                .any(|existing| path_matches_filter(&root, existing))
            {
                minimal_roots.push(root);
            }
        }

        Ok(if minimal_roots.is_empty() {
            Self::None
        } else {
            Self::Roots(minimal_roots)
        })
    }

    fn append_sql(&self, sql: &mut String, values: &mut Vec<Value>, column: &str) {
        match self {
            Self::All => {}
            Self::None => sql.push_str(" AND 0"),
            Self::Roots(roots) => {
                sql.push_str(" AND (");
                for (index, root) in roots.iter().enumerate() {
                    if index != 0 {
                        sql.push_str(" OR ");
                    }
                    let root_parameter = values.len() + 1;
                    let upper_parameter = root_parameter + 1;
                    let descendant_parameter = root_parameter + 2;
                    sql.push_str(&format!(
                        "({column} >= ?{root_parameter} COLLATE BINARY AND \
                         {column} < ?{upper_parameter} COLLATE BINARY AND \
                         ({column} = ?{root_parameter} COLLATE BINARY OR \
                          {column} >= ?{descendant_parameter} COLLATE BINARY))"
                    ));
                    values.push(Value::Text(root.clone()));
                    values.push(Value::Text(format!("{root}0")));
                    values.push(Value::Text(format!("{root}/")));
                }
                sql.push(')');
            }
        }
    }
}

#[cfg(test)]
mod mod_tests;
