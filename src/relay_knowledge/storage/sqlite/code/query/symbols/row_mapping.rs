use crate::domain::RepositoryCodeRange;

use crate::storage::sqlite::code::query::rows::SymbolRow;

pub(super) fn row_to_symbol(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolRow> {
    Ok(SymbolRow {
        symbol_snapshot_id: row.get(0)?,
        canonical_symbol_id: row.get(1)?,
        file_id: row.get(2)?,
        path: row.get(3)?,
        language_id: row.get(4)?,
        signature: row.get(5)?,
        doc_comment: row.get(6)?,
        byte_range: RepositoryCodeRange {
            start: row.get(7)?,
            end: row.get(8)?,
        },
        line_range: RepositoryCodeRange {
            start: row.get(9)?,
            end: row.get(10)?,
        },
        name: row.get(11)?,
        qualified_name: row.get(12)?,
        kind: row.get(13)?,
        is_generated: row.get::<_, i64>(14)? != 0,
        previous_symbol_context_start: row.get(15)?,
    })
}

#[cfg(test)]
#[path = "row_mapping_tests.rs"]
mod tests;
