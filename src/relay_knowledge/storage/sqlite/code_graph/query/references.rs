use rusqlite::{Connection, Row, params};

use crate::{
    domain::{
        CodeExtractionMetadata, CodeReferenceFields, CodeReferenceKind, CodeReferenceRecord,
        CodeResolutionState,
    },
    storage::{CodeReferenceSearchRequest, StorageError},
};

use super::common::{
    RawRange, extraction, invalid_code_metadata, normalize_filter, parse_scope, validate_limit,
};

pub(in crate::storage::sqlite) fn search_references(
    connection: &mut Connection,
    request: CodeReferenceSearchRequest,
) -> Result<Vec<CodeReferenceRecord>, StorageError> {
    validate_limit("code reference search limit", request.limit)?;
    let scope = normalize_filter("source_scope", request.source_scope)?;
    let path = normalize_filter("code_path", request.path)?;
    let symbol_text = normalize_filter("symbol_text", request.symbol_text)?;
    let target_symbol_id = normalize_filter("target_symbol_id", request.target_symbol_id)?;
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, reference_id, symbol_text, kind, start_byte,
               end_byte, start_line, end_line, resolution_state, target_symbol_id,
               grammar_version, query_name, query_version, node_kind, capture_kind
        FROM code_references
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND (?2 IS NULL OR path = ?2)
          AND (?3 IS NULL OR lower(symbol_text) LIKE '%' || lower(?3) || '%')
          AND (?4 IS NULL OR target_symbol_id = ?4)
          AND created_graph_version <= ?5
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC,
                 start_line ASC, reference_id ASC
        LIMIT ?6
        ",
    )?;
    let rows = statement.query_map(
        params![
            scope.as_deref(),
            path.as_deref(),
            symbol_text.as_deref(),
            target_symbol_id.as_deref(),
            request.graph_version.get(),
            request.limit
        ],
        row_to_reference,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
        .into_iter()
        .map(RawReference::into_record)
        .collect()
}

struct RawReference {
    source_scope: String,
    path: String,
    reference_id: String,
    symbol_text: String,
    kind: String,
    range: RawRange,
    resolution_state: String,
    target_symbol_id: Option<String>,
    extraction: CodeExtractionMetadata,
}

impl RawReference {
    fn into_record(self) -> Result<CodeReferenceRecord, StorageError> {
        CodeReferenceRecord::new(CodeReferenceFields {
            reference_id: self.reference_id,
            source_scope: parse_scope(self.source_scope)?,
            path: self.path,
            symbol_text: self.symbol_text,
            kind: parse_reference_kind(&self.kind)?,
            range: self.range.into_range()?,
            resolution_state: parse_resolution_state(&self.resolution_state)?,
            target_symbol_id: self.target_symbol_id,
            extraction: self.extraction,
        })
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
    }
}

fn row_to_reference(row: &Row<'_>) -> rusqlite::Result<RawReference> {
    Ok(RawReference {
        source_scope: row.get(0)?,
        path: row.get(1)?,
        reference_id: row.get(2)?,
        symbol_text: row.get(3)?,
        kind: row.get(4)?,
        range: RawRange::from_row(row, 5)?,
        resolution_state: row.get(9)?,
        target_symbol_id: row.get(10)?,
        extraction: extraction(
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
            row.get(14)?,
            row.get(15)?,
        ),
    })
}

fn parse_reference_kind(value: &str) -> Result<CodeReferenceKind, StorageError> {
    match value {
        "call" => Ok(CodeReferenceKind::Call),
        "type" => Ok(CodeReferenceKind::Type),
        "import" => Ok(CodeReferenceKind::Import),
        "implementation" => Ok(CodeReferenceKind::Implementation),
        _ => Err(invalid_code_metadata(format!(
            "unknown code reference kind '{value}'"
        ))),
    }
}

fn parse_resolution_state(value: &str) -> Result<CodeResolutionState, StorageError> {
    match value {
        "unresolved" => Ok(CodeResolutionState::Unresolved),
        "ambiguous" => Ok(CodeResolutionState::Ambiguous),
        "resolved" => Ok(CodeResolutionState::Resolved),
        _ => Err(invalid_code_metadata(format!(
            "unknown code resolution state '{value}'"
        ))),
    }
}

#[cfg(test)]
#[path = "references_tests.rs"]
mod references_tests;
