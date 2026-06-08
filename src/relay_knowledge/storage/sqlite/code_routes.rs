use rusqlite::{Transaction, params};

use crate::{domain::CodeRouteRecord, storage::StorageError};

use super::SearchDocumentInserter;

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[CodeRouteRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_routes (
            repository_id, source_scope, route_id, file_id, path, language_id,
            url, http_method, handler_name, handler_symbol_snapshot_id, framework,
            line_start, line_end
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for record in records {
        statement.execute(params![
            record.repository_id,
            record.source_scope,
            record.route_id,
            record.file_id,
            record.path,
            record.language_id,
            record.url,
            record.http_method,
            record.handler_name,
            record.handler_symbol_snapshot_id,
            record.framework,
            record.line_range.start,
            record.line_range.end,
        ])?;
        search_documents.insert(
            &record.source_scope,
            "route",
            &record.route_id,
            &record.path,
            &record.language_id,
            [
                record.url.as_str(),
                record.http_method.as_str(),
                record.handler_name.as_str(),
                record.framework.as_str(),
                record.path.as_str(),
            ],
        )?;
    }

    Ok(())
}
