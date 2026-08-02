use rusqlite::Row;

use crate::{
    domain::{CodeExtractionMetadata, CodeRange, DomainError, SourceScope},
    storage::StorageError,
};

pub(super) struct RawRange {
    start_byte: u32,
    end_byte: u32,
    start_line: u32,
    end_line: u32,
}

impl RawRange {
    pub(super) fn from_row(row: &Row<'_>, start_index: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            start_byte: row.get(start_index)?,
            end_byte: row.get(start_index + 1)?,
            start_line: row.get(start_index + 2)?,
            end_line: row.get(start_index + 3)?,
        })
    }

    pub(super) fn into_range(self) -> Result<CodeRange, StorageError> {
        CodeRange::new(
            self.start_byte,
            self.end_byte,
            self.start_line,
            self.end_line,
        )
        .map_err(domain_error)
    }
}

pub(super) fn extraction(
    grammar_version: String,
    query_name: String,
    query_version: String,
    node_kind: String,
    capture_kind: String,
) -> CodeExtractionMetadata {
    CodeExtractionMetadata {
        grammar_version,
        query_name,
        query_version,
        node_kind,
        capture_kind,
    }
}

pub(super) fn optional_extraction(
    grammar_version: Option<String>,
    query_name: Option<String>,
    query_version: Option<String>,
    node_kind: Option<String>,
    capture_kind: Option<String>,
) -> Option<CodeExtractionMetadata> {
    Some(CodeExtractionMetadata {
        grammar_version: grammar_version?,
        query_name: query_name?,
        query_version: query_version?,
        node_kind: node_kind?,
        capture_kind: capture_kind?,
    })
}

pub(super) fn normalize_filter(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<String>, StorageError> {
    value
        .map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(StorageError::InvalidInput(format!(
                    "{field} filter must not be empty"
                )));
            }
            if trimmed.contains('\0') {
                return Err(StorageError::InvalidInput(format!(
                    "{field} filter must not contain NUL bytes"
                )));
            }

            Ok(trimmed.to_owned())
        })
        .transpose()
}

pub(super) fn validate_limit(label: &'static str, limit: usize) -> Result<(), StorageError> {
    if limit == 0 {
        return Err(StorageError::InvalidInput(format!(
            "{label} must be greater than zero"
        )));
    }

    Ok(())
}

pub(super) fn parse_scope(value: String) -> Result<SourceScope, StorageError> {
    SourceScope::parse(value).map_err(domain_error)
}

pub(super) fn invalid_code_metadata(message: String) -> StorageError {
    StorageError::InvalidInput(format!("{message} in code graph metadata"))
}

fn domain_error(error: DomainError) -> StorageError {
    StorageError::InvalidInput(error.to_string())
}

#[cfg(test)]
#[path = "common_tests.rs"]
mod common_tests;
