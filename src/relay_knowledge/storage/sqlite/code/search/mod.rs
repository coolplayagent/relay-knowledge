//! Code search-document persistence, cleanup, and identifier expansion.

use std::sync::OnceLock;

use rusqlite::{limits::Limit, params, params_from_iter, types::Value};

use crate::storage::StorageError;

pub(crate) struct SearchDocumentInserter<'transaction> {
    transaction: &'transaction rusqlite::Transaction<'transaction>,
    document_batch_size: usize,
    documents: Vec<PendingSearchDocument>,
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

const SEARCH_DOCUMENT_INSERT_BATCH_SIZE: usize = 1_024;
const SEARCH_DOCUMENT_COLUMN_COUNT: usize = 6;
const SEARCH_DOCUMENT_INSERT_BIND_COUNT: usize =
    SEARCH_DOCUMENT_INSERT_BATCH_SIZE * SEARCH_DOCUMENT_COLUMN_COUNT;
const SEARCH_DOCUMENT_INSERT_ROW: &str = "(?, ?, ?, ?, ?, ?)";
const SEARCH_DOCUMENT_ROW_STORAGE_OVERHEAD_BYTES: usize = 32;
static SEARCH_DOCUMENT_INSERT_FULL_SQL: OnceLock<String> = OnceLock::new();
const _: () = assert!(SEARCH_DOCUMENT_INSERT_BIND_COUNT == 6_144);
const SEARCH_DOCUMENT_INTERVAL_COUNT_SQL: &str = "
    SELECT count(*)
    FROM code_repository_search
    WHERE rowid > ?1 AND rowid <= ?2
";
const SEARCH_DOCUMENT_MAX_ROWID_EXISTS_SQL: &str = "
    SELECT EXISTS (
        SELECT 1
        FROM code_repository_search
        WHERE rowid = ?1
    )
";
const SEARCH_DOCUMENT_METADATA_INSERT_SQL: &str = "
    INSERT INTO code_repository_search_metadata (
        source_scope, document_kind, record_id, path, search_rowid
    )
    SELECT source_scope, document_kind, record_id, path, rowid
    FROM code_repository_search
    WHERE rowid > ?1 AND rowid <= ?2
";

/// Requires an FTS row to be owned by the exact indexed metadata identity.
///
/// Every production FTS `MATCH` statement uses the canonical virtual-table name, so one shared
/// correlated predicate keeps legacy or imported unowned rows outside every serving surface.
pub(in crate::storage::sqlite::code) const EXACT_SEARCH_OWNER_PREDICATE_SQL: &str = "
AND EXISTS (
    SELECT 1
    FROM code_repository_search_metadata exact_search_owner
    WHERE exact_search_owner.search_rowid = code_repository_search.rowid
      AND exact_search_owner.source_scope = code_repository_search.source_scope
      AND exact_search_owner.document_kind = code_repository_search.document_kind
      AND exact_search_owner.record_id = code_repository_search.record_id
      AND exact_search_owner.path = code_repository_search.path
)";

impl<'transaction> SearchDocumentInserter<'transaction> {
    pub(crate) fn new(
        transaction: &'transaction rusqlite::Transaction<'_>,
    ) -> Result<Self, StorageError> {
        let variable_limit = usize::try_from(
            transaction.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER),
        )
        .map_err(|_| {
            StorageError::Invariant(
                "SQLite reported a negative variable limit for search-document persistence"
                    .to_owned(),
            )
        })?;
        let rows_within_variable_limit = variable_limit / SEARCH_DOCUMENT_COLUMN_COUNT;
        if rows_within_variable_limit == 0 {
            return Err(StorageError::Invariant(format!(
                "SQLite variable limit {variable_limit} cannot admit one {SEARCH_DOCUMENT_COLUMN_COUNT}-column search-document row"
            )));
        }
        let document_batch_size = SEARCH_DOCUMENT_INSERT_BATCH_SIZE.min(rows_within_variable_limit);
        Ok(Self {
            transaction,
            document_batch_size,
            documents: Vec::with_capacity(document_batch_size),
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
        if self.documents.len() == self.document_batch_size {
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
        let pending_document_count = self.documents.len();
        let pending_document_count_i64 = i64::try_from(pending_document_count).map_err(|_| {
            StorageError::Invariant(format!(
                "search-document batch size {pending_document_count} does not fit in a SQLite row count"
            ))
        })?;
        require_consecutive_search_rowids(self.transaction)?;
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
        let inserted_search_document_count = if pending_document_count == self.document_batch_size {
            if self.document_batch_size == SEARCH_DOCUMENT_INSERT_BATCH_SIZE {
                let sql = SEARCH_DOCUMENT_INSERT_FULL_SQL
                    .get_or_init(|| search_document_insert_sql(SEARCH_DOCUMENT_INSERT_BATCH_SIZE));
                let mut statement = self.transaction.prepare_cached(sql)?;
                statement.execute(params_from_iter(values))?
            } else {
                let sql = search_document_insert_sql(self.document_batch_size);
                let mut statement = self.transaction.prepare_cached(&sql)?;
                statement.execute(params_from_iter(values))?
            }
        } else {
            let sql = search_document_insert_sql(pending_document_count);
            let mut statement = self.transaction.prepare(&sql)?;
            statement.execute(params_from_iter(values))?
        };
        if inserted_search_document_count != pending_document_count {
            return Err(StorageError::Invariant(format!(
                "search-document FTS insert affected {inserted_search_document_count} rows for {pending_document_count} pending documents"
            )));
        }
        let last_search_rowid = self.transaction.last_insert_rowid();
        let first_exclusive_search_rowid = last_search_rowid
            .checked_sub(pending_document_count_i64)
            .ok_or_else(|| {
                StorageError::Invariant(format!(
                    "search-document rowid interval underflows before {last_search_rowid} for {pending_document_count} pending documents"
                ))
            })?;
        let interval_document_count = {
            let mut statement = self
                .transaction
                .prepare_cached(SEARCH_DOCUMENT_INTERVAL_COUNT_SQL)?;
            statement.query_row(
                params![first_exclusive_search_rowid, last_search_rowid],
                |row| row.get::<_, i64>(0),
            )?
        };
        if interval_document_count != pending_document_count_i64 {
            return Err(StorageError::Invariant(format!(
                "search-document FTS rowid interval ({first_exclusive_search_rowid}, {last_search_rowid}] contains {interval_document_count} rows for {pending_document_count} pending documents"
            )));
        }
        let inserted_metadata_count = {
            let mut statement = self
                .transaction
                .prepare_cached(SEARCH_DOCUMENT_METADATA_INSERT_SQL)?;
            statement.execute(params![first_exclusive_search_rowid, last_search_rowid])?
        };
        if inserted_metadata_count != pending_document_count {
            return Err(StorageError::Invariant(format!(
                "search-document metadata insert affected {inserted_metadata_count} rows for {pending_document_count} pending documents"
            )));
        }
        self.documents.clear();

        Ok(())
    }
}

/// Rejects the one SQLite rowid state where automatic allocation is not consecutive.
///
/// FTS5 normal-content tables allocate an omitted rowid through their content rowid table. An
/// existing `INT64_MAX` row makes SQLite switch from `max + 1` to random unused rowids, so any
/// owner that derives a bounded inserted interval from `last_insert_rowid()` must call this in the
/// same transaction before its FTS insert.
pub(in crate::storage::sqlite::code) fn require_consecutive_search_rowids(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), StorageError> {
    let maximum_rowid_exists = {
        let mut statement = transaction.prepare_cached(SEARCH_DOCUMENT_MAX_ROWID_EXISTS_SQL)?;
        statement.query_row(params![i64::MAX], |row| row.get::<_, bool>(0))?
    };
    if maximum_rowid_exists {
        return Err(StorageError::Invariant(
            "code search FTS contains the maximum SQLite rowid; automatic rowid allocation would not be consecutive"
                .to_owned(),
        ));
    }

    Ok(())
}

fn search_document_insert_sql(row_count: usize) -> String {
    let placeholders = std::iter::repeat_n(SEARCH_DOCUMENT_INSERT_ROW, row_count)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "
        INSERT INTO code_repository_search (
            source_scope, document_kind, record_id, path, language_id, content
        )
        VALUES {placeholders}
        "
    )
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

pub(super) fn insert_search_document<'a>(
    transaction: &rusqlite::Transaction<'_>,
    source_scope: &str,
    document_kind: &str,
    record_id: &str,
    path: &str,
    language_id: &str,
    fields: impl IntoIterator<Item = &'a str>,
) -> Result<(), StorageError> {
    let mut inserter = SearchDocumentInserter::new(transaction)?;
    inserter.insert(
        source_scope,
        document_kind,
        record_id,
        path,
        language_id,
        fields,
    )?;
    inserter.finish()
}

pub(in crate::storage::sqlite::code) fn search_document_content<'a>(
    document_kind: &str,
    fields: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut content = String::new();
    let mut symbol_terms = Vec::new();
    search_document_content_into(&mut content, &mut symbol_terms, document_kind, fields);

    content
}

/// Text fields persisted by one grouped reference owner across its group, FTS, and metadata rows.
pub(in crate::storage::sqlite::code) struct ReferenceSearchGroupStorage<'value> {
    pub source_scope: &'value str,
    pub group_id: &'value str,
    pub name: &'value str,
    pub reference_kind: &'value str,
    pub path: &'value str,
    pub target_hint: &'value str,
    pub language_id: &'value str,
}

impl ReferenceSearchGroupStorage<'_> {
    /// Returns a conservative byte upper bound for the three authoritative owner rows.
    pub(in crate::storage::sqlite::code) fn persisted_byte_upper_bound(&self) -> Option<usize> {
        let document_kind = "reference";
        let content = search_document_content(
            document_kind,
            [self.name, self.reference_kind, self.target_hint, self.path],
        );
        [
            self.source_scope.len().checked_mul(3)?,
            document_kind.len().checked_mul(2)?,
            self.group_id.len().checked_mul(3)?,
            self.path.len().checked_mul(3)?,
            self.language_id.len().checked_mul(2)?,
            self.name.len(),
            self.reference_kind.len(),
            self.target_hint.len(),
            content.len(),
            SEARCH_DOCUMENT_ROW_STORAGE_OVERHEAD_BYTES.checked_mul(3)?,
            16,
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add)
    }
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
#[path = "mod_tests.rs"]
mod mod_tests;
