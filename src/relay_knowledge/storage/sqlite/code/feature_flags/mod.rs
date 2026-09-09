//! Feature-flag graph persistence, filtering, and ranked query ownership.

use rusqlite::{Connection, Transaction, params};

use crate::{
    domain::{CodeFeatureFlagGraph, CodeFeatureFlagRecord, CodeFeatureFlagRequest},
    storage::StorageError,
};

use super::{SearchDocumentInserter, query::hits::required_repository};

pub(super) fn insert_records(
    transaction: &Transaction<'_>,
    records: &[CodeFeatureFlagRecord],
) -> Result<(), StorageError> {
    let mut statement = transaction.prepare(
        "
        INSERT OR REPLACE INTO code_repository_feature_flags (
            repository_id, source_scope, feature_flag_id, usage_id, file_id, path, language_id,
            name, source_kind, source_key, edge_kind, confidence_basis_points, confidence_tier,
            byte_start, byte_end, line_start, line_end, excerpt, metadata_json
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        ",
    )?;
    let mut search_documents = SearchDocumentInserter::new(transaction)?;
    for record in records {
        statement.execute(params![
            record.repository_id,
            record.source_scope,
            record.feature_flag_id,
            record.usage_id,
            record.file_id,
            record.path,
            record.language_id,
            record.name,
            record.source_kind,
            record.source_key,
            record.edge_kind,
            record.confidence_basis_points,
            record.confidence_tier,
            record.byte_range.start,
            record.byte_range.end,
            record.line_range.start,
            record.line_range.end,
            record.excerpt,
            serde_json::to_string(&record.metadata)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
        ])?;
        search_documents.insert(
            &record.source_scope,
            "feature_flag",
            &record.usage_id,
            &record.path,
            &record.language_id,
            [
                record.name.as_str(),
                record.source_kind.as_str(),
                record.source_key.as_str(),
                record.edge_kind.as_str(),
                record.excerpt.as_str(),
                record.path.as_str(),
            ],
        )?;
    }
    search_documents.finish()?;

    Ok(())
}

pub(super) fn search(
    connection: &mut Connection,
    request: CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let status = required_repository(connection, &request.repository)?;
    super::schema::require_feature_flag_query_index(connection)?;
    super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        knowledge::search(connection, &status, &request)
    })
}

pub(super) fn search_scope(
    connection: &mut Connection,
    source_scope: &str,
    request: CodeFeatureFlagRequest,
) -> Result<Vec<CodeFeatureFlagGraph>, StorageError> {
    let status = super::status::repository_scope_status_by_source_scope(connection, source_scope)?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository source scope '{source_scope}' is not indexed"
            ))
        })?;
    super::schema::require_feature_flag_query_index(connection)?;
    super::super::connection_runtime::retry::retry_sqlite_transient(|| {
        knowledge::search(connection, &status, &request)
    })
}

mod candidates;
mod filters;
mod knowledge;

#[cfg(test)]
mod mod_tests;

#[cfg(test)]
pub(super) mod test_support;
