//! Silent-update service-operator state, updates, and row decoding.

use rusqlite::{Connection, Row, params};

use crate::{
    domain::{ServiceOperatorState, ServiceOperatorStatus},
    storage::{ServiceOperatorUpdate, StorageError},
};

pub(in crate::storage::sqlite) fn service_operator_status(
    connection: &Connection,
) -> Result<ServiceOperatorStatus, StorageError> {
    connection
        .query_row(
            "
            SELECT state, silent_updates_enabled, allowed_scopes_json, last_run_at_ms,
                   next_retry_at_ms, last_error, updated_at_ms
            FROM service_operator_state
            WHERE id = 1
            ",
            [],
            service_operator_from_row,
        )
        .map_err(StorageError::from)
}

pub(in crate::storage::sqlite) fn update_service_operator(
    connection: &Connection,
    request: ServiceOperatorUpdate,
) -> Result<ServiceOperatorStatus, StorageError> {
    let allowed_scopes_json = serde_json::to_string(&request.allowed_scopes)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    connection.execute(
        "
        UPDATE service_operator_state
        SET state = ?1,
            silent_updates_enabled = ?2,
            allowed_scopes_json = ?3,
            last_error = ?4,
            updated_at_ms = ?5
        WHERE id = 1
        ",
        params![
            request.state.as_str(),
            request.silent_updates_enabled,
            allowed_scopes_json,
            request.last_error,
            request.now_ms,
        ],
    )?;

    service_operator_status(connection)
}

fn service_operator_from_row(row: &Row<'_>) -> rusqlite::Result<ServiceOperatorStatus> {
    let allowed_scopes_json: String = row.get(2)?;
    let allowed_scopes = serde_json::from_str(&allowed_scopes_json).unwrap_or_default();
    Ok(ServiceOperatorStatus {
        state: parse_service_operator_state(row.get::<_, String>(0)?),
        silent_updates_enabled: row.get(1)?,
        allowed_scopes,
        last_run_at_ms: row.get(3)?,
        next_retry_at_ms: row.get(4)?,
        last_error: row.get(5)?,
        updated_at_ms: row.get(6)?,
    })
}

fn parse_service_operator_state(value: String) -> ServiceOperatorState {
    ServiceOperatorState::parse(&value).unwrap_or(ServiceOperatorState::Failed)
}

#[cfg(test)]
#[path = "service_operator_tests.rs"]
mod tests;
