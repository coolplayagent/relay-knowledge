//! Bounded keyset pagination across module, declaration, component and usage streams.
use super::*;
use crate::storage::sqlite::maven::reactor;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    fingerprint: u64,
    phase: u8,
    key: String,
}

enum Fact {
    Module(SoftwareBuildTarget),
    Edge(SoftwareRelationship),
    Component(SoftwareComponent),
    Usage(SoftwareDependencyUsage),
}

pub(super) fn page(
    connection: &Connection,
    scope: &str,
    request: &SoftwareGlobalRequest,
) -> Result<ProjectionSlices, StorageError> {
    let snapshot = if connection.is_autocommit() {
        Some(connection.unchecked_transaction()?)
    } else {
        None
    };
    let connection = snapshot
        .as_ref()
        .map_or(connection, |transaction| transaction);
    reactor::require_complete(connection, scope)?;
    let phases = if request.kind == SoftwareGlobalKind::Modules {
        2
    } else {
        4
    };
    let version = connection
        .query_row(
            "SELECT projected_graph_version FROM software_global_status WHERE source_scope=?",
            [scope],
            |row| row.get::<_, u64>(0),
        )
        .optional()?
        .unwrap_or(0);
    // source_scope already incorporates the Git tree/worktree overlay identity. A new scope,
    // filter, kind or projection version must never continue a previous result stream.
    let identity = serde_json::to_vec(&(
        scope,
        request.kind,
        &request.repository.path_filters,
        &request.repository.language_filters,
        version,
        SOFTWARE_PROJECTION_SCHEMA_VERSION,
    ))
    .map_err(|error| StorageError::Invariant(error.to_string()))?;
    let fingerprint = crate::identity::stable_hash64(&identity);
    let mut cursor = Cursor::read(request.cursor.as_deref(), fingerprint, phases)?;
    let mut slices = ProjectionSlices::default();
    let mut remaining = request.limit;
    while cursor.phase < phases {
        let rows = rows(connection, scope, request, &cursor, remaining + 1)?;
        for fact in rows {
            if remaining == 0 {
                slices.next_cursor = Some(cursor.encode()?);
                return Ok(slices);
            }
            cursor.key = match fact {
                Fact::Module(value) => {
                    let key = value.target_id.clone();
                    slices.build_targets.push(value);
                    key
                }
                Fact::Edge(value) => {
                    let key = value.relationship_id.clone();
                    slices.relationships.push(value);
                    key
                }
                Fact::Component(value) => {
                    let key = value.component_id.clone();
                    slices.components.push(value);
                    key
                }
                Fact::Usage(value) => {
                    let key = value.usage_id.clone();
                    slices.dependency_usages.push(value);
                    key
                }
            };
            remaining -= 1;
        }
        cursor.phase += 1;
        cursor.key.clear();
    }
    Ok(slices)
}

fn rows(
    connection: &Connection,
    scope: &str,
    request: &SoftwareGlobalRequest,
    cursor: &Cursor,
    limit: usize,
) -> Result<Vec<Fact>, StorageError> {
    match cursor.phase {
        0 => Ok(
            reactor::read_page(connection, scope, request, &cursor.key, limit, false)?
                .into_iter()
                .map(Fact::Module)
                .collect(),
        ),
        1 => Ok(
            reactor::read_page(connection, scope, request, &cursor.key, limit, true)?
                .into_iter()
                .map(Fact::Edge)
                .collect(),
        ),
        _ => evidence_page(connection, scope, request, cursor, limit),
    }
}

fn evidence_page(
    connection: &Connection,
    scope: &str,
    request: &SoftwareGlobalRequest,
    cursor: &Cursor,
    limit: usize,
) -> Result<Vec<Fact>, StorageError> {
    let path_filter = path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter =
        language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let (columns, table, id) = if cursor.phase == 2 {
        (
            "component_id, repository_id, source_scope, ecosystem, name, requirement, resolved_version, dependency_group, source_kind, relationship_state, language_id, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
            "software_components",
            "component_id",
        )
    } else {
        (
            "usage_id, component_id, repository_id, source_scope, ecosystem, package_name, language_id, module, target_hint, resolution_state, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
            "software_dependency_usages",
            "usage_id",
        )
    };
    let sql = format!(
        "SELECT {columns} FROM {table} WHERE source_scope=? AND {id}>? {path_filter} {language_filter} ORDER BY {id} LIMIT ?"
    );
    let mut values = vec![Value::Text(scope.into()), Value::Text(cursor.key.clone())];
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&sql)?;
    statement
        .query_map(params_from_iter(values), |row| {
            if cursor.phase == 2 {
                component_from_row(row).map(Fact::Component)
            } else {
                dependency_usage::usage_from_row(row).map(Fact::Usage)
            }
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

impl Cursor {
    fn read(token: Option<&str>, fingerprint: u64, phases: u8) -> Result<Self, StorageError> {
        let Some(token) = token else {
            return Ok(Self {
                fingerprint,
                phase: 0,
                key: String::new(),
            });
        };
        let payload = token.strip_prefix("sw1:").ok_or_else(invalid_cursor)?;
        if payload.len() > 4092 || payload.len() % 2 != 0 || !payload.is_ascii() {
            return Err(invalid_cursor());
        }
        let bytes = (0..payload.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&payload[index..index + 2], 16).map_err(|_| invalid_cursor())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cursor: Self = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
        if cursor.fingerprint != fingerprint || cursor.phase >= phases || cursor.key.len() > 1024 {
            return Err(invalid_cursor());
        }
        Ok(cursor)
    }

    fn encode(&self) -> Result<String, StorageError> {
        let json =
            serde_json::to_vec(self).map_err(|error| StorageError::Invariant(error.to_string()))?;
        let token = format!(
            "sw1:{}",
            json.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        if token.len() > 4096 {
            return Err(invalid_cursor());
        }
        Ok(token)
    }
}

fn invalid_cursor() -> StorageError {
    StorageError::InvalidInput(
        "invalid software cursor or changed snapshot/query; restart pagination without cursor"
            .into(),
    )
}

#[cfg(test)]
#[path = "dependencies_tests.rs"]
mod tests;
