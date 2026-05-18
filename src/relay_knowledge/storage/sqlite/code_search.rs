use rusqlite::params;

use crate::storage::StorageError;

pub(crate) struct SearchDocumentInserter<'transaction> {
    statement: rusqlite::Statement<'transaction>,
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

        Ok(Self { statement })
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
        insert_search_document_with_statement(
            &mut self.statement,
            source_scope,
            document_kind,
            record_id,
            path,
            language_id,
            fields,
        )
    }
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

fn insert_search_document_with_statement<'a>(
    statement: &mut rusqlite::Statement<'_>,
    source_scope: &str,
    document_kind: &str,
    record_id: &str,
    path: &str,
    language_id: &str,
    fields: impl IntoIterator<Item = &'a str>,
) -> Result<(), StorageError> {
    let content = search_document_content(document_kind, fields);
    statement.execute(params![
        source_scope,
        document_kind,
        record_id,
        path,
        language_id,
        content
    ])?;

    Ok(())
}

fn search_document_content<'a>(
    document_kind: &str,
    fields: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut content = String::new();
    let mut symbol_search_fields = 0usize;
    let mut symbol_terms = Vec::new();
    for field in fields {
        if field.trim().is_empty() {
            continue;
        }
        append_search_field(&mut content, field);
        if document_kind == "symbol" && symbol_search_fields < 2 {
            push_identifier_search_terms(field, &mut symbol_terms);
        }
        symbol_search_fields += 1;
    }

    if document_kind == "symbol" && !symbol_terms.is_empty() {
        symbol_terms.sort();
        symbol_terms.dedup();
        for term in symbol_terms {
            append_search_field(&mut content, &term);
        }
    }

    content
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
    use super::search_document_content;

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
}
