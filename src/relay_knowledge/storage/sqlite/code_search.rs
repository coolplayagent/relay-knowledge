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
    let fields = fields
        .into_iter()
        .filter(|field| !field.trim().is_empty())
        .collect::<Vec<_>>();
    let mut content = fields.join(" ");
    if document_kind == "symbol" {
        let terms =
            identifier_search_terms(&fields.iter().take(2).copied().collect::<Vec<_>>().join(" "));
        if !terms.is_empty() {
            content.push(' ');
            content.push_str(&terms.join(" "));
        }
    }
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

fn identifier_search_terms(content: &str) -> Vec<String> {
    let mut terms = Vec::new();
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
        terms.extend(camel_case_terms(token));
    }
    terms.sort();
    terms.dedup();

    terms
}

fn camel_case_terms(token: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut start = 0;
    let mut previous: Option<char> = None;
    let chars = token.char_indices().collect::<Vec<_>>();
    for (index, (byte_index, character)) in chars.iter().enumerate() {
        let next = chars.get(index + 1).map(|(_, next)| *next);
        let starts_upper_word = character.is_ascii_uppercase()
            && previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || next.is_some_and(|next| next.is_ascii_lowercase())
            });
        if *byte_index > start && starts_upper_word {
            terms.push(token[start..*byte_index].to_ascii_lowercase());
            start = *byte_index;
        }
        previous = Some(*character);
    }
    if start < token.len() {
        terms.push(token[start..].to_ascii_lowercase());
    }

    terms
}
