//! Encodes and validates bounded incremental-summary checkpoint receipts.

use crate::{
    domain::{CodeIncrementalSummaryReceipt, CodeIndexResourceBudget},
    storage::StorageError,
};

const MAX_INCREMENTAL_SUMMARY_BYTES: usize = 4 * 1024;

#[cfg(test)]
#[path = "checkpoint_receipt_tests.rs"]
mod tests;

pub(in crate::storage::sqlite::code) fn encode(
    receipt: &CodeIncrementalSummaryReceipt,
) -> Result<String, StorageError> {
    receipt
        .validate()
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let encoded = serde_json::to_string(receipt)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    if encoded.len() > MAX_INCREMENTAL_SUMMARY_BYTES {
        return Err(StorageError::CapacityExceeded(
            "incremental summary receipt exceeds its durable checkpoint bound".to_owned(),
        ));
    }
    Ok(encoded)
}

pub(in crate::storage::sqlite::code) fn decode(
    encoded: Option<String>,
    column: usize,
    budget: CodeIndexResourceBudget,
) -> rusqlite::Result<Option<CodeIncrementalSummaryReceipt>> {
    let Some(encoded) = encoded else {
        return Ok(None);
    };
    if encoded.len() > MAX_INCREMENTAL_SUMMARY_BYTES {
        return Err(conversion_error(
            column,
            "incremental summary receipt exceeds its durable checkpoint bound",
        ));
    }
    let receipt = serde_json::from_str::<CodeIncrementalSummaryReceipt>(&encoded)
        .map_err(|error| conversion_error(column, error))?;
    receipt
        .validate()
        .map_err(|error| conversion_error(column, error))?;
    let canonical =
        serde_json::to_string(&receipt).map_err(|error| conversion_error(column, error))?;
    if canonical != encoded {
        return Err(conversion_error(
            column,
            "incremental summary receipt is not canonical",
        ));
    }
    let file_budget = budget
        .max_files_per_batch
        .checked_mul(receipt.batch_count)
        .ok_or_else(|| conversion_error(column, "incremental file budget overflowed"))?;
    let row_budget = budget
        .max_rows_per_batch
        .checked_mul(receipt.batch_count)
        .ok_or_else(|| conversion_error(column, "incremental row budget overflowed"))?;
    if receipt.affected_path_count > file_budget
        || receipt.blob_read_count > file_budget
        || receipt.sqlite_write_count > row_budget
        || encoded.len() > budget.max_bytes_per_batch
    {
        return Err(conversion_error(
            column,
            "incremental summary receipt exceeds its frozen resource budget",
        ));
    }
    Ok(Some(receipt))
}

fn conversion_error(column: usize, error: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(error.to_string())),
    )
}
