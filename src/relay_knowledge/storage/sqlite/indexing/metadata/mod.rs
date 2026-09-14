//! Persistent index metadata codecs and stable refresh identity primitives.

use std::collections::BTreeSet;

use crate::{
    domain::{IndexKind, IndexModality, IndexState, IndexStatus},
    storage::{IndexRefreshTaskState, StorageError},
};

pub(super) use crate::identity::stable_hash64;

pub(crate) fn parse_json_array(value: String) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(&value).map_err(|error| {
        StorageError::InvalidInput(format!("invalid mutation log JSON array: {error}"))
    })
}

pub(crate) fn source_hash(source_scope: &str, source_path: Option<&str>, content: &str) -> String {
    let mut input = Vec::new();
    append_hash_part(&mut input, source_scope);
    append_hash_part(&mut input, source_path.unwrap_or(""));
    append_hash_part(&mut input, content);

    format!("{:016x}", stable_hash64(&input))
}

pub(crate) fn json_array(values: impl IntoIterator<Item = String>) -> Result<String, StorageError> {
    let unique = values.into_iter().collect::<BTreeSet<_>>();

    serde_json::to_string(&unique.into_iter().collect::<Vec<_>>())
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
}
pub(super) fn parse_index_kind(value: &str) -> Result<IndexKind, StorageError> {
    match value {
        "bm25" => Ok(IndexKind::Bm25),
        "semantic" => Ok(IndexKind::Semantic),
        "vector" => Ok(IndexKind::Vector),
        _ => Err(invalid_index_metadata(format!(
            "unknown index kind '{value}'"
        ))),
    }
}

pub(super) fn parse_index_modality(value: &str) -> Result<IndexModality, StorageError> {
    match value {
        "text" => Ok(IndexModality::Text),
        "image" => Ok(IndexModality::Image),
        "layout" => Ok(IndexModality::Layout),
        "table" => Ok(IndexModality::Table),
        _ => Err(invalid_index_metadata(format!(
            "unknown index modality '{value}'"
        ))),
    }
}

pub(super) fn parse_index_state(value: &str) -> Result<IndexState, StorageError> {
    match value {
        "fresh" => Ok(IndexState::Fresh),
        "stale" => Ok(IndexState::Stale),
        "failed" => Ok(IndexState::Failed),
        "paused" => Ok(IndexState::Paused),
        _ => Err(invalid_index_metadata(format!(
            "unknown index state '{value}'"
        ))),
    }
}

pub(super) fn parse_task_state(value: &str) -> Result<IndexRefreshTaskState, StorageError> {
    match value {
        "queued" => Ok(IndexRefreshTaskState::Queued),
        "running" => Ok(IndexRefreshTaskState::Running),
        "succeeded" => Ok(IndexRefreshTaskState::Succeeded),
        "retrying" => Ok(IndexRefreshTaskState::Retrying),
        "failed" => Ok(IndexRefreshTaskState::Failed),
        "dead_letter" => Ok(IndexRefreshTaskState::DeadLetter),
        _ => Err(invalid_index_metadata(format!(
            "unknown index refresh task state '{value}'"
        ))),
    }
}

pub(super) fn validate_required_index_statuses(
    statuses: &[IndexStatus],
) -> Result<(), StorageError> {
    for kind in IndexKind::ALL {
        if !statuses.iter().any(|status| status.kind == kind) {
            return Err(invalid_index_metadata(format!(
                "required index status row for '{}' is missing",
                kind.as_str()
            )));
        }
    }

    Ok(())
}

pub(super) fn invalid_index_metadata(message: String) -> StorageError {
    StorageError::InvalidInput(format!("{message} in storage metadata"))
}

pub(super) fn invalid_to_sqlite(error: StorageError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

pub(super) fn append_hash_part(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&(value.len() as u64).to_le_bytes());
    input.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
