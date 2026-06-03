use rusqlite::{params, params_from_iter, types::Value};

use crate::storage::StorageError;

pub(crate) struct SearchDocumentInserter<'transaction> {
    statement: rusqlite::Statement<'transaction>,
    metadata_statement: rusqlite::Statement<'transaction>,
    content: String,
    symbol_terms: Vec<String>,
}

impl<'transaction> SearchDocumentInserter<'transaction> {
    pub(crate) fn new(
        transaction: &'transaction rusqlite::Transaction<'_>,
    ) -> Result<Self, StorageError> {
        let statement = transaction.prepare(
            "
            INSERT INTO code_repository_search (
                source_scope, document_kind, record_id, path, language_id, content
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
        )?;
        let metadata_statement = transaction.prepare(
            "
            INSERT OR REPLACE INTO code_repository_search_metadata (
                source_scope, document_kind, record_id, path, search_rowid
            )
            VALUES (?1, ?2, ?3, ?4, ?5)
            ",
        )?;

        Ok(Self {
            statement,
            metadata_statement,
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
        let search_rowid = self.statement.insert(params![
            source_scope,
            document_kind,
            record_id,
            path,
            language_id,
            self.content.as_str()
        ])?;
        self.metadata_statement.execute(params![
            source_scope,
            document_kind,
            record_id,
            path,
            search_rowid
        ])?;

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
    )
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
        if document_kind == "symbol" && symbol_search_fields < 2 {
            push_identifier_search_terms(field, symbol_terms);
        }
        symbol_search_fields += 1;
    }

    if document_kind == "symbol" && !symbol_terms.is_empty() {
        symbol_terms.sort();
        symbol_terms.dedup();
        for term in symbol_terms.iter() {
            append_search_field(content, term);
        }
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
mod tests {
    use super::{search_document_content, search_document_content_into};

    #[test]
    fn symbol_search_content_preserves_identifier_expansion() {
        let content = search_document_content(
            "symbol",
            [
                "NewLRUCache",
                "",
                "leveldb::NewLRUCache",
                "function",
                "db/cache.cc",
            ],
        );

        assert_eq!(
            content,
            "NewLRUCache leveldb::NewLRUCache function db/cache.cc cache leveldb lru new newlrucache"
        );
    }

    #[test]
    fn non_symbol_search_content_keeps_only_nonempty_fields() {
        let content = search_document_content("chunk", ["", "body text", "  ", "src/lib.rs"]);

        assert_eq!(content, "body text src/lib.rs");
    }

    #[test]
    fn reusable_search_content_buffers_do_not_leak_previous_terms() {
        let mut content = String::from("stale content");
        let mut symbol_terms = vec!["stale".to_owned()];
        search_document_content_into(
            &mut content,
            &mut symbol_terms,
            "symbol",
            ["GraphIndex", "relay_knowledge::GraphIndex"],
        );
        assert_eq!(
            content,
            "GraphIndex relay_knowledge::GraphIndex graph graphindex index knowledge relay relay_knowledge"
        );

        search_document_content_into(&mut content, &mut symbol_terms, "chunk", ["new chunk"]);
        assert_eq!(content, "new chunk");
        assert!(symbol_terms.is_empty());
    }
}
