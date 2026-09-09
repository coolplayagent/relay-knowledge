//! Snapshot-local Java receiver proof before any candidate window is admitted.
use crate::{domain::CodeRepositoryStatus, storage::StorageError};
use rusqlite::{Connection, params};

pub(super) fn admission(
    connection: &Connection,
    status: &CodeRepositoryStatus,
    scope: &str,
) -> Result<String, StorageError> {
    // Request-level paths select returned usages, not the provider inventory.
    let filesystem = status
        .last_indexed_commit
        .as_deref()
        .is_some_and(|identity| identity.starts_with("filesystem:"));
    let full_paths = if filesystem {
        // Empty filesystem scope uses discovered source roots, not the whole repository.
        status
            .path_filters
            .iter()
            .any(|path| crate::domain::normalize_filesystem_path_filter(path) == ".")
    } else {
        status.path_filters.is_empty() || status.path_filters.iter().any(|path| path == ".")
    };
    let authorized = full_paths
        && (status.language_filters.is_empty()
            || status
                .language_filters
                .iter()
                .any(|language| language == "java"));
    let complete = authorized
        && connection.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM code_repository_java_namespaces
             WHERE source_scope=?1 AND complete=0)
         AND NOT EXISTS(SELECT 1 FROM code_repository_files file
             WHERE file.source_scope=?1 AND file.language_id='java'
             AND NOT EXISTS(SELECT 1 FROM code_repository_java_namespaces namespace
                 WHERE namespace.source_scope=file.source_scope AND namespace.path=file.path))",
            params![scope],
            |row| row.get::<_, bool>(0),
        )?;
    if !complete {
        return Ok(
            "json_extract(flag.metadata_json,'$.java_implicit_platform') IS NULL".to_owned(),
        );
    }
    Ok("(json_extract(flag.metadata_json,'$.java_implicit_platform') IS NULL OR (
        EXISTS(SELECT 1 FROM code_repository_java_namespaces namespace
            WHERE namespace.source_scope=flag.source_scope AND namespace.path=flag.path
              AND namespace.complete=1
        AND NOT EXISTS(SELECT 1 FROM code_repository_java_types provider
            WHERE provider.source_scope=flag.source_scope
              AND provider.package=namespace.package
              AND provider.type_name=json_extract(flag.metadata_json,'$.java_implicit_platform.type_name')))))".to_owned())
}

#[cfg(test)]
#[path = "java_platform_tests.rs"]
mod tests;
