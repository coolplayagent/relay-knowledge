use rusqlite::{Connection, Row, params};

use crate::{
    domain::{CodeExtractionMetadata, CodeSymbolKind, CodeSymbolRecord},
    storage::{CodeSymbolSearchRequest, StorageError},
};

use super::common::{
    RawRange, extraction, invalid_code_metadata, normalize_filter, parse_scope, validate_limit,
};

pub(in crate::storage::sqlite) fn search_symbols(
    connection: &mut Connection,
    request: CodeSymbolSearchRequest,
) -> Result<Vec<CodeSymbolRecord>, StorageError> {
    validate_limit("code symbol search limit", request.limit)?;
    let scope = normalize_filter("source_scope", request.source_scope)?;
    let path = normalize_filter("code_path", request.path)?;
    let name = normalize_filter("symbol_name", request.name)?;
    let mut statement = connection.prepare(
        "
        SELECT source_scope, path, symbol_id, name, kind, start_byte, end_byte,
               start_line, end_line, grammar_version, query_name, query_version,
               node_kind, capture_kind
        FROM code_symbols
        WHERE (?1 IS NULL OR source_scope = ?1)
          AND (?2 IS NULL OR path = ?2)
          AND (?3 IS NULL OR lower(name) LIKE '%' || lower(?3) || '%')
          AND created_graph_version <= ?4
        ORDER BY created_graph_version DESC, source_scope ASC, path ASC,
                 start_line ASC, symbol_id ASC
        LIMIT ?5
        ",
    )?;
    let rows = statement.query_map(
        params![
            scope.as_deref(),
            path.as_deref(),
            name.as_deref(),
            request.graph_version.get(),
            request.limit
        ],
        row_to_symbol,
    )?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)?
        .into_iter()
        .map(RawSymbol::into_record)
        .collect()
}

struct RawSymbol {
    source_scope: String,
    path: String,
    symbol_id: String,
    name: String,
    kind: String,
    range: RawRange,
    extraction: CodeExtractionMetadata,
}

impl RawSymbol {
    fn into_record(self) -> Result<CodeSymbolRecord, StorageError> {
        CodeSymbolRecord::new(
            self.symbol_id,
            parse_scope(self.source_scope)?,
            self.path,
            self.name,
            parse_symbol_kind(&self.kind)?,
            self.range.into_range()?,
            self.extraction,
        )
        .map_err(|error| StorageError::InvalidInput(error.to_string()))
    }
}

fn row_to_symbol(row: &Row<'_>) -> rusqlite::Result<RawSymbol> {
    Ok(RawSymbol {
        source_scope: row.get(0)?,
        path: row.get(1)?,
        symbol_id: row.get(2)?,
        name: row.get(3)?,
        kind: row.get(4)?,
        range: RawRange::from_row(row, 5)?,
        extraction: extraction(
            row.get(9)?,
            row.get(10)?,
            row.get(11)?,
            row.get(12)?,
            row.get(13)?,
        ),
    })
}

fn parse_symbol_kind(value: &str) -> Result<CodeSymbolKind, StorageError> {
    match value {
        "function" => Ok(CodeSymbolKind::Function),
        "method" => Ok(CodeSymbolKind::Method),
        "class" => Ok(CodeSymbolKind::Class),
        "interface" => Ok(CodeSymbolKind::Interface),
        "module" => Ok(CodeSymbolKind::Module),
        "type" => Ok(CodeSymbolKind::Type),
        "constant" => Ok(CodeSymbolKind::Constant),
        "field" => Ok(CodeSymbolKind::Field),
        "variable" => Ok(CodeSymbolKind::Variable),
        "enum_member" => Ok(CodeSymbolKind::EnumMember),
        _ => Err(invalid_code_metadata(format!(
            "unknown code symbol kind '{value}'"
        ))),
    }
}

#[cfg(test)]
#[path = "symbols_tests.rs"]
mod symbols_tests;
