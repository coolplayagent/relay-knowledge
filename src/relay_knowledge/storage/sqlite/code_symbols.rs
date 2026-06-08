use rusqlite::{Transaction, params};

use crate::{
    domain::{RepositoryCodeSymbolRecord, SymbolRole},
    storage::StorageError,
};

use super::SearchDocumentInserter;

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[RepositoryCodeSymbolRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT INTO code_repository_symbols (
            repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id,
            file_id, path, language_id, name,
            qualified_name, kind, signature, doc_comment, byte_start, byte_end,
            line_start, line_end, symbol_role_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for symbol in records {
        let symbol_role_json = symbol_role_json(&symbol.symbol_role)?;
        statement.execute(params![
            symbol.repository_id,
            symbol.source_scope,
            symbol.symbol_snapshot_id,
            symbol.canonical_symbol_id,
            symbol.file_id,
            symbol.path,
            symbol.language_id,
            symbol.name,
            symbol.qualified_name,
            symbol.kind,
            symbol.signature,
            symbol.doc_comment,
            symbol.byte_range.start,
            symbol.byte_range.end,
            symbol.line_range.start,
            symbol.line_range.end,
            symbol_role_json,
        ])?;
        let (role_kind, role_url, role_method) = symbol_role_search_fields(&symbol.symbol_role);
        search_documents.insert(
            &symbol.source_scope,
            "symbol",
            &symbol.symbol_snapshot_id,
            &symbol.path,
            &symbol.language_id,
            [
                symbol.name.as_str(),
                symbol.qualified_name.as_str(),
                symbol.kind.as_str(),
                symbol.signature.as_str(),
                symbol.doc_comment.as_deref().unwrap_or_default(),
                symbol.path.as_str(),
                role_kind,
                role_url,
                role_method,
            ],
        )?;
    }

    Ok(())
}

fn symbol_role_json(role: &Option<SymbolRole>) -> Result<Option<String>, StorageError> {
    role.as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn symbol_role_search_fields(role: &Option<SymbolRole>) -> (&str, &str, &str) {
    match role {
        Some(SymbolRole::RouteHandler { url, http_method }) => {
            ("route_handler", url.as_str(), http_method.as_str())
        }
        None => ("", "", ""),
    }
}
