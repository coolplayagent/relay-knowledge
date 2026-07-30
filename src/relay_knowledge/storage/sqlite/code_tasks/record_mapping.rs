use rusqlite::Row;

use crate::domain::{
    CodeIndexCheckpoint, CodeIndexResourceBudget, CodeIndexTaskRecord, CodeIndexTaskState,
};

use super::super::code_status::parse_json_list;

const TASK_RECORD_COLUMNS: &str = "
    task_id, repository_id, alias, ref_selector, resolved_commit_sha, tree_hash,
    source_scope, path_filters_json, language_filters_json, mode_json, state,
    lease_owner, lease_expires_at_ms, attempt_count, next_retry_at_ms,
    input_fingerprint, resource_budget_json, payload_json, last_error_kind,
    last_error_message, created_at_ms, updated_at_ms
";

pub(super) fn task_select_sql(where_clause: &str) -> String {
    format!(
        "
        SELECT {TASK_RECORD_COLUMNS}
        FROM code_repository_index_tasks
        {where_clause}
        "
    )
}

pub(super) fn task_update_returning_sql(update_sql: &str) -> String {
    format!("{update_sql} RETURNING {TASK_RECORD_COLUMNS}")
}

pub(super) fn task_from_row(row: &Row<'_>) -> rusqlite::Result<CodeIndexTaskRecord> {
    let state = parse_task_state(row.get::<_, String>(10)?.as_str(), 10)?;
    let mode = serde_json::from_str(row.get::<_, String>(9)?.as_str()).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let resource_budget =
        serde_json::from_str(row.get::<_, String>(16)?.as_str()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                16,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    Ok(CodeIndexTaskRecord {
        task_id: row.get(0)?,
        repository_id: row.get(1)?,
        alias: row.get(2)?,
        ref_selector: row.get(3)?,
        resolved_commit_sha: row.get(4)?,
        tree_hash: row.get(5)?,
        source_scope: row.get(6)?,
        path_filters: parse_json_list(row.get(7)?)?,
        language_filters: parse_json_list(row.get(8)?)?,
        mode,
        state,
        lease_owner: row.get(11)?,
        lease_expires_at_ms: row.get(12)?,
        attempt_count: row.get(13)?,
        next_retry_at_ms: row.get(14)?,
        input_fingerprint: row.get(15)?,
        resource_budget,
        payload_json: row.get(17)?,
        last_error_kind: row.get(18)?,
        last_error_message: row.get(19)?,
        created_at_ms: row.get(20)?,
        updated_at_ms: row.get(21)?,
    })
}

pub(super) fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<CodeIndexCheckpoint> {
    let resource_budget =
        serde_json::from_str::<CodeIndexResourceBudget>(row.get::<_, String>(11)?.as_str())
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
    Ok(CodeIndexCheckpoint {
        repository_id: row.get(0)?,
        source_scope: row.get(1)?,
        state: row.get(2)?,
        total_path_count: row.get(3)?,
        parsed_file_count: row.get(4)?,
        committed_file_count: row.get(5)?,
        committed_symbol_count: row.get(6)?,
        committed_reference_count: row.get(7)?,
        committed_chunk_count: row.get(8)?,
        batch_count: row.get(9)?,
        last_path: row.get(10)?,
        resource_budget,
        updated_at_ms: row.get(12)?,
    })
}

fn parse_task_state(value: &str, column: usize) -> rusqlite::Result<CodeIndexTaskState> {
    CodeIndexTaskState::parse(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
