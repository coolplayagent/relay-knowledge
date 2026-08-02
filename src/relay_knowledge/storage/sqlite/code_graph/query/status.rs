use rusqlite::Connection;

use crate::{
    domain::{CodeParseStatus, CodeParseStatusCounts},
    storage::StorageError,
};

use super::common::invalid_code_metadata;

pub(in crate::storage::sqlite) fn parse_status_counts(
    connection: &Connection,
) -> Result<CodeParseStatusCounts, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT parse_status, COUNT(*)
        FROM code_files
        GROUP BY parse_status
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
    })?;
    let mut counts = CodeParseStatusCounts::default();
    for row in rows {
        let (status, count) = row.map_err(StorageError::from)?;
        match parse_status(&status)? {
            CodeParseStatus::Parsed => counts.parsed = count,
            CodeParseStatus::Partial => counts.partial = count,
            CodeParseStatus::TextOnly => counts.text_only = count,
            CodeParseStatus::Failed => counts.failed = count,
        }
    }

    Ok(counts)
}

fn parse_status(value: &str) -> Result<CodeParseStatus, StorageError> {
    match value {
        "parsed" => Ok(CodeParseStatus::Parsed),
        "partial" => Ok(CodeParseStatus::Partial),
        "text_only" => Ok(CodeParseStatus::TextOnly),
        "failed" => Ok(CodeParseStatus::Failed),
        _ => Err(invalid_code_metadata(format!(
            "unknown code parse status '{value}'"
        ))),
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod status_tests;
