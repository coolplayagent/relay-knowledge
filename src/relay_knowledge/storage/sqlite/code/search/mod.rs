//! Code search-document persistence, cleanup, and identifier expansion.

use rusqlite::{params, params_from_iter, types::Value};

use crate::storage::StorageError;

pub(crate) struct SearchDocumentInserter<'transaction> {
    transaction: &'transaction rusqlite::Transaction<'transaction>,
    documents: Vec<PendingSearchDocument>,
    last_search_rowid: i64,
    content: String,
    symbol_terms: Vec<String>,
}

struct PendingSearchDocument {
    source_scope: String,
    document_kind: String,
    record_id: String,
    path: String,
    language_id: String,
    content: String,
}

const SEARCH_DOCUMENT_INSERT_BATCH_SIZE: usize = 256;
const SEARCH_DOCUMENT_COLUMN_COUNT: usize = 6;

impl<'transaction> SearchDocumentInserter<'transaction> {
    pub(crate) fn new(
        transaction: &'transaction rusqlite::Transaction<'_>,
    ) -> Result<Self, StorageError> {
        let last_search_rowid = transaction.query_row(
            "SELECT coalesce(max(search_rowid), 0) FROM code_repository_search_metadata",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(Self {
            transaction,
            documents: Vec::with_capacity(SEARCH_DOCUMENT_INSERT_BATCH_SIZE),
            last_search_rowid,
            content: String::new(),
            symbol_terms: Vec::new(),
        })
    }

    pub(crate) fn insert<'a>(
        &mut self,
        source_scope: &str,
        document_kind: &str,
        record_id: &str,
        path: &str,
        language_id: &str,
        fields: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), StorageError> {
        search_document_content_into(
            &mut self.content,
            &mut self.symbol_terms,
            document_kind,
            fields,
        );
        self.documents.push(PendingSearchDocument {
            source_scope: source_scope.to_owned(),
            document_kind: document_kind.to_owned(),
            record_id: record_id.to_owned(),
            path: path.to_owned(),
            language_id: language_id.to_owned(),
            content: std::mem::take(&mut self.content),
        });
        if self.documents.len() == SEARCH_DOCUMENT_INSERT_BATCH_SIZE {
            self.flush()?;
        }

        Ok(())
    }

    pub(crate) fn finish(mut self) -> Result<(), StorageError> {
        self.flush()
    }

    fn flush(&mut self) -> Result<(), StorageError> {
        if self.documents.is_empty() {
            return Ok(());
        }
        let previous_search_rowid = self.last_search_rowid;
        let placeholders = std::iter::repeat_n("(?, ?, ?, ?, ?, ?)", self.documents.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Vec::with_capacity(self.documents.len() * SEARCH_DOCUMENT_COLUMN_COUNT);
        for document in &self.documents {
            values.extend([
                Value::Text(document.source_scope.clone()),
                Value::Text(document.document_kind.clone()),
                Value::Text(document.record_id.clone()),
                Value::Text(document.path.clone()),
                Value::Text(document.language_id.clone()),
                Value::Text(document.content.clone()),
            ]);
        }
        self.transaction.execute(
            &format!(
                "
                INSERT INTO code_repository_search (
                    source_scope, document_kind, record_id, path, language_id, content
                )
                VALUES {placeholders}
                "
            ),
            params_from_iter(values),
        )?;
        let last_search_rowid = self.transaction.last_insert_rowid();
        self.transaction.execute(
            "
            INSERT OR REPLACE INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            SELECT source_scope, document_kind, record_id, path, rowid
            FROM code_repository_search
            WHERE rowid > ?1 AND rowid <= ?2
            ",
            params![previous_search_rowid, last_search_rowid],
        )?;
        self.last_search_rowid = last_search_rowid;
        self.documents.clear();

        Ok(())
    }
}

pub(super) fn delete_search_documents_for_scope(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE rowid IN (
            SELECT search_rowid
            FROM code_repository_search_metadata
            WHERE source_scope = ?1
        )
        ",
        params![source_scope],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_search_metadata WHERE source_scope = ?1",
        params![source_scope],
    )?;
    transaction.execute(
        "DELETE FROM code_repository_search WHERE source_scope = ?1",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn backfill_search_metadata_for_scope(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        INSERT OR IGNORE INTO code_repository_search_metadata (
            source_scope, document_kind, record_id, path, search_rowid
        )
        SELECT source_scope, document_kind, record_id, path, rowid
        FROM code_repository_search
        WHERE source_scope = ?1
        ",
        params![source_scope],
    )?;

    Ok(())
}

pub(super) fn delete_search_documents_for_kind(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    document_kind: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "
        DELETE FROM code_repository_search
        WHERE rowid IN (
            SELECT search_rowid
            FROM code_repository_search_metadata
            WHERE source_scope = ?1 AND document_kind = ?2
        )
        ",
        params![source_scope, document_kind],
    )?;
    transaction.execute(
        "
        DELETE FROM code_repository_search_metadata
        WHERE source_scope = ?1 AND document_kind = ?2
        ",
        params![source_scope, document_kind],
    )?;

    Ok(())
}

pub(super) fn delete_search_documents_for_paths<'path>(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    paths: impl IntoIterator<Item = &'path str>,
) -> Result<(), StorageError> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    if paths.is_empty() {
        return Ok(());
    }
    for path_chunk in paths.chunks(500) {
        let placeholders = std::iter::repeat_n("?", path_chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let mut values = Vec::with_capacity(path_chunk.len() + 1);
        values.push(Value::Text(source_scope.to_owned()));
        values.extend(
            path_chunk
                .iter()
                .map(|path| Value::Text((*path).to_owned())),
        );
        transaction.execute(
            &format!(
                "
                DELETE FROM code_repository_search
                WHERE rowid IN (
                    SELECT search_rowid
                    FROM code_repository_search_metadata
                    WHERE source_scope = ? AND path IN ({placeholders})
                )
                "
            ),
            params_from_iter(values.clone()),
        )?;
        transaction.execute(
            &format!(
                "
                DELETE FROM code_repository_search_metadata
                WHERE source_scope = ? AND path IN ({placeholders})
                "
            ),
            params_from_iter(values),
        )?;
    }

    Ok(())
}

#[cfg(test)]
fn search_document_content<'a>(
    document_kind: &str,
    fields: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut content = String::new();
    let mut symbol_terms = Vec::new();
    search_document_content_into(&mut content, &mut symbol_terms, document_kind, fields);

    content
}

fn search_document_content_into<'a>(
    content: &mut String,
    symbol_terms: &mut Vec<String>,
    document_kind: &str,
    fields: impl IntoIterator<Item = &'a str>,
) {
    content.clear();
    symbol_terms.clear();
    let mut symbol_search_fields = 0usize;
    for field in fields {
        if field.trim().is_empty() {
            continue;
        }
        append_search_field(content, field);
        if search_field_expands_identifier_terms(document_kind, symbol_search_fields) {
            push_identifier_search_terms(field, symbol_terms);
        }
        symbol_search_fields += 1;
    }

    if !symbol_terms.is_empty() {
        symbol_terms.sort();
        symbol_terms.dedup();
        for term in symbol_terms.iter() {
            append_search_field(content, term);
        }
    }
}

fn search_field_expands_identifier_terms(document_kind: &str, field_index: usize) -> bool {
    match document_kind {
        "symbol" => field_index < 2,
        "route" => field_index == 3,
        _ => false,
    }
}

fn append_search_field(content: &mut String, field: &str) {
    let separator_bytes = usize::from(!content.is_empty());
    content.reserve(separator_bytes.saturating_add(field.len()));
    if separator_bytes > 0 {
        content.push(' ');
    }
    content.push_str(field);
}

fn push_identifier_search_terms(content: &str, terms: &mut Vec<String>) {
    for token in
        content.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
    {
        if token.is_empty() {
            continue;
        }
        terms.extend(
            token
                .split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_ascii_lowercase),
        );
        push_camel_case_terms(token, terms);
    }
}

fn push_camel_case_terms(token: &str, terms: &mut Vec<String>) {
    let mut start = 0;
    let mut previous: Option<char> = None;
    let mut characters = token.char_indices().peekable();
    while let Some((byte_index, character)) = characters.next() {
        let next = characters.peek().map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if byte_index > start && starts_upper_word {
            terms.push(token[start..byte_index].to_ascii_lowercase());
            start = byte_index;
        }
        previous = Some(character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }
}

#[cfg(test)]
mod mod_tests;
