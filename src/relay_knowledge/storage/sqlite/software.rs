use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareComponent, SoftwareComponentInput,
        SoftwareGlobalKind, SoftwareGlobalProjection, SoftwareGlobalRequest, SoftwareGlobalStatus,
        SoftwareSdkUsage, SoftwareSdkUsageInput,
    },
    storage::StorageError,
};

use super::super::current_graph_version;
use super::code_query_scope;

pub(super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS software_components (
            component_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            ecosystem TEXT NOT NULL,
            name TEXT NOT NULL,
            requirement TEXT,
            resolved_version TEXT,
            dependency_group TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            relationship_state TEXT NOT NULL,
            language_id TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_components_scope
            ON software_components(source_scope, language_id, ecosystem, name);

        CREATE TABLE IF NOT EXISTS software_sdk_usages (
            usage_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            language_id TEXT NOT NULL,
            module TEXT NOT NULL,
            target_hint TEXT,
            resolution_state TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_sdk_usages_scope
            ON software_sdk_usages(source_scope, language_id, module);

        CREATE TABLE IF NOT EXISTS software_global_status (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            projected_graph_version INTEGER NOT NULL,
            stale INTEGER NOT NULL,
            component_count INTEGER NOT NULL,
            sdk_usage_count INTEGER NOT NULL,
            last_error TEXT
        );
        ",
    )?;

    Ok(())
}

pub(super) fn refresh_projection(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<SoftwareGlobalProjection, StorageError> {
    let graph_version = current_graph_version(connection)?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM software_components WHERE source_scope = ?1",
        params![source_scope],
    )?;
    transaction.execute(
        "DELETE FROM software_sdk_usages WHERE source_scope = ?1",
        params![source_scope],
    )?;

    let components = dependency_components(&transaction, source_scope, graph_version)?;
    for component in &components {
        insert_component(&transaction, component)?;
    }

    let sdk_usages = unresolved_sdk_usages(&transaction, source_scope, graph_version)?;
    for usage in &sdk_usages {
        insert_sdk_usage(&transaction, usage)?;
    }

    let repository_id = repository_id_for_scope(&transaction, source_scope)?
        .unwrap_or_else(|| "unknown".to_owned());
    let status = SoftwareGlobalStatus {
        repository_id,
        source_scope: source_scope.to_owned(),
        projected_graph_version: graph_version,
        stale: false,
        component_count: components.len(),
        sdk_usage_count: sdk_usages.len(),
        last_error: None,
    };
    upsert_status(&transaction, &status)?;
    transaction.commit()?;

    Ok(SoftwareGlobalProjection {
        status,
        components,
        sdk_usages,
    })
}

pub(super) fn projection(
    connection: &mut Connection,
    request: SoftwareGlobalRequest,
) -> Result<SoftwareGlobalProjection, StorageError> {
    let source_scope = source_scope_for_request(connection, &request)?;
    projection_for_scope(connection, &source_scope, request)
}

pub(super) fn projection_for_scope(
    connection: &mut Connection,
    source_scope: &str,
    request: SoftwareGlobalRequest,
) -> Result<SoftwareGlobalProjection, StorageError> {
    let status =
        status_for_scope(connection, source_scope)?.unwrap_or_else(|| SoftwareGlobalStatus {
            repository_id: repository_id_for_scope(connection, source_scope)
                .ok()
                .flatten()
                .unwrap_or_else(|| request.repository.repository.clone()),
            source_scope: source_scope.to_owned(),
            projected_graph_version: GraphVersion::ZERO,
            stale: true,
            component_count: 0,
            sdk_usage_count: 0,
            last_error: Some("software global projection has not been refreshed".to_owned()),
        });
    let components = match request.kind {
        SoftwareGlobalKind::Dependencies | SoftwareGlobalKind::All => {
            components_for_scope(connection, source_scope, &request, request.limit)?
        }
        SoftwareGlobalKind::Sdks => Vec::new(),
    };
    let sdk_usages = match request.kind {
        SoftwareGlobalKind::Sdks => {
            sdk_usages_for_scope(connection, source_scope, &request, request.limit)?
        }
        SoftwareGlobalKind::All => {
            let remaining = request.limit.saturating_sub(components.len());
            if remaining == 0 {
                Vec::new()
            } else {
                sdk_usages_for_scope(connection, source_scope, &request, remaining)?
            }
        }
        SoftwareGlobalKind::Dependencies => Vec::new(),
    };

    Ok(SoftwareGlobalProjection {
        status,
        components,
        sdk_usages,
    })
}

fn dependency_components(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<Vec<SoftwareComponent>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT repository_id, source_scope, ecosystem, package_name, requirement,
               resolved_version, dependency_group, source_kind, is_lockfile,
               language_id, path, line_start, line_end
        FROM code_repository_dependencies
        WHERE source_scope = ?1
        ORDER BY ecosystem ASC, package_name ASC, is_lockfile DESC, path ASC, line_start ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        let is_lockfile = row.get::<_, i64>(8)? != 0;
        Ok(SoftwareComponentInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            ecosystem: row.get(2)?,
            name: row.get(3)?,
            requirement: row.get(4)?,
            resolved_version: row.get(5)?,
            dependency_group: row.get(6)?,
            source_kind: row.get(7)?,
            relationship_state: if is_lockfile { "locked" } else { "declared" }.to_owned(),
            language_id: row.get(9)?,
            evidence_path: row.get(10)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(11)?,
                end: row.get(12)?,
            },
            confidence_basis_points: 10_000,
            created_graph_version: graph_version,
        })
    })?;

    rows.map(|row| {
        row.map_err(StorageError::from).and_then(|input| {
            SoftwareComponent::new(input)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))
        })
    })
    .collect()
}

fn unresolved_sdk_usages(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<Vec<SoftwareSdkUsage>, StorageError> {
    let mut statement = connection.prepare(
        "
        SELECT imports.repository_id, imports.source_scope, files.language_id,
               imports.module, imports.target_hint, imports.resolution_state,
               imports.path, imports.line_start, imports.line_end,
               imports.confidence_basis_points
        FROM code_repository_imports imports
        JOIN code_repository_files files
          ON files.source_scope = imports.source_scope
         AND files.path = imports.path
        WHERE imports.source_scope = ?1
          AND imports.resolution_state IN ('unresolved', 'ambiguous', 'external')
        ORDER BY files.language_id ASC, imports.module ASC, imports.path ASC
        ",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(SoftwareSdkUsageInput {
            repository_id: row.get(0)?,
            source_scope: row.get(1)?,
            language_id: row.get(2)?,
            module: row.get(3)?,
            target_hint: row.get(4)?,
            resolution_state: row.get(5)?,
            evidence_path: row.get(6)?,
            evidence_line_range: RepositoryCodeRange {
                start: row.get(7)?,
                end: row.get(8)?,
            },
            confidence_basis_points: row.get(9)?,
            created_graph_version: graph_version,
        })
    })?;

    rows.map(|row| {
        row.map_err(StorageError::from).and_then(|input| {
            SoftwareSdkUsage::new(input)
                .map_err(|error| StorageError::InvalidInput(error.to_string()))
        })
    })
    .collect()
}

fn insert_component(
    connection: &Connection,
    component: &SoftwareComponent,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_components (
            component_id, repository_id, source_scope, ecosystem, name, requirement,
            resolved_version, dependency_group, source_kind, relationship_state,
            language_id, evidence_path, evidence_line_start, evidence_line_end,
            confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
        params![
            component.component_id,
            component.repository_id,
            component.source_scope,
            component.ecosystem,
            component.name,
            component.requirement,
            component.resolved_version,
            component.dependency_group,
            component.source_kind,
            component.relationship_state,
            component.language_id,
            component.evidence_path,
            component.evidence_line_range.start,
            component.evidence_line_range.end,
            component.confidence_basis_points,
            component.created_graph_version.get(),
        ],
    )?;

    Ok(())
}

fn insert_sdk_usage(connection: &Connection, usage: &SoftwareSdkUsage) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT OR REPLACE INTO software_sdk_usages (
            usage_id, repository_id, source_scope, language_id, module, target_hint,
            resolution_state, evidence_path, evidence_line_start, evidence_line_end,
            confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
        params![
            usage.usage_id,
            usage.repository_id,
            usage.source_scope,
            usage.language_id,
            usage.module,
            usage.target_hint,
            usage.resolution_state,
            usage.evidence_path,
            usage.evidence_line_range.start,
            usage.evidence_line_range.end,
            usage.confidence_basis_points,
            usage.created_graph_version.get(),
        ],
    )?;

    Ok(())
}

fn upsert_status(
    connection: &Connection,
    status: &SoftwareGlobalStatus,
) -> Result<(), StorageError> {
    connection.execute(
        "
        INSERT INTO software_global_status (
            source_scope, repository_id, projected_graph_version, stale,
            component_count, sdk_usage_count, last_error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            projected_graph_version = excluded.projected_graph_version,
            stale = excluded.stale,
            component_count = excluded.component_count,
            sdk_usage_count = excluded.sdk_usage_count,
            last_error = excluded.last_error
        ",
        params![
            status.source_scope,
            status.repository_id,
            status.projected_graph_version.get(),
            if status.stale { 1_i64 } else { 0_i64 },
            status.component_count,
            status.sdk_usage_count,
            status.last_error,
        ],
    )?;

    Ok(())
}

fn source_scope_for_request(
    connection: &mut Connection,
    request: &SoftwareGlobalRequest,
) -> Result<String, StorageError> {
    if let Ok(source_scope) = exact_source_scope_for_request(connection, request) {
        return Ok(source_scope);
    }
    let repository_id = repository_id_for_request(connection, &request.repository.repository)?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "code repository '{}' is not registered",
                request.repository.repository
            ))
        })?;
    let mut statement = connection.prepare(
        "
        SELECT scope.source_scope, scope.path_filters_json, scope.language_filters_json
        FROM code_repository_scopes scope
        WHERE scope.repository_id = ?1
          AND scope.resolved_commit_sha = ?2
        ORDER BY scope.path_filters_json ASC, scope.language_filters_json ASC,
                 scope.source_scope ASC
        ",
    )?;
    let candidates = statement.query_map(
        params![repository_id, request.repository.ref_selector],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    for candidate in candidates {
        let (source_scope, path_filters_json, language_filters_json) = candidate?;
        let path_filters = parse_filter_json(&path_filters_json)?;
        let language_filters = parse_filter_json(&language_filters_json)?;
        if code_query_scope::selector_filters_fit_indexed_scope(
            &path_filters,
            &language_filters,
            &request.repository.path_filters,
            &request.repository.language_filters,
        ) {
            return Ok(source_scope);
        }
    }

    Err(StorageError::InvalidInput(format!(
        "code repository '{}' does not have an indexed software projection scope for ref '{}'",
        request.repository.repository, request.repository.ref_selector
    )))
}

fn repository_id_for_request(
    connection: &Connection,
    repository: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id
            FROM (
                SELECT repository_id, 0 AS precedence
                FROM code_repositories
                WHERE repository_id = ?1
                UNION ALL
                SELECT repository_id, 1 AS precedence
                FROM code_repository_aliases
                WHERE alias = ?1
            )
            ORDER BY precedence ASC
            LIMIT 1
            ",
            params![repository],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn parse_filter_json(value: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(value).map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn path_filter_sql_for_column(column: &str, filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
        .map(|_| format!("({column} = ? OR {column} LIKE ? ESCAPE '\\')"))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

fn language_filter_sql_for_column(column: &str, filters: &[String]) -> String {
    let clauses = filters
        .iter()
        .map(|_| format!("{column} = ?"))
        .collect::<Vec<_>>();
    if clauses.is_empty() {
        String::new()
    } else {
        format!("AND ({})", clauses.join(" OR "))
    }
}

fn push_path_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    for filter in filters
        .iter()
        .filter_map(|filter| normalized_sql_path_filter(filter))
    {
        values.push(Value::Text(filter.clone()));
        values.push(Value::Text(format!("{}/%", escape_sql_like(&filter))));
    }
}

fn push_language_filter_values(values: &mut Vec<Value>, filters: &[String]) {
    values.extend(filters.iter().cloned().map(Value::Text));
}

fn normalized_sql_path_filter(filter: &str) -> Option<String> {
    let mut filter = filter.trim_end_matches(['/', '\\']);
    while let Some(stripped) = filter.strip_prefix("./") {
        filter = stripped;
    }

    (!filter.is_empty() && filter != ".").then(|| filter.to_owned())
}

fn escape_sql_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn source_scope_filter_error(request: &SoftwareGlobalRequest) -> StorageError {
    StorageError::InvalidInput(format!(
        "code repository '{}' does not have an indexed software projection scope for ref '{}'",
        request.repository.repository, request.repository.ref_selector
    ))
}

fn exact_source_scope_for_request(
    connection: &mut Connection,
    request: &SoftwareGlobalRequest,
) -> Result<String, StorageError> {
    let path_filters_json = serde_json::to_string(&request.repository.path_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let language_filters_json = serde_json::to_string(&request.repository.language_filters)
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let repository_id = repository_id_for_request(connection, &request.repository.repository)?
        .ok_or_else(|| source_scope_filter_error(request))?;
    connection
        .query_row(
            "
        SELECT scope.source_scope
        FROM code_repository_scopes scope
        WHERE scope.repository_id = ?1
          AND scope.resolved_commit_sha = ?2
          AND scope.path_filters_json = ?3
          AND scope.language_filters_json = ?4
        ORDER BY scope.source_scope ASC
        LIMIT 1
        ",
            params![
                repository_id,
                request.repository.ref_selector,
                path_filters_json,
                language_filters_json,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| source_scope_filter_error(request))
}

fn repository_id_for_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id
            FROM code_repository_scopes
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| row.get(0),
        )
        .optional()
        .map_err(StorageError::from)
}

fn status_for_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<Option<SoftwareGlobalStatus>, StorageError> {
    connection
        .query_row(
            "
            SELECT repository_id, source_scope, projected_graph_version, stale,
                   component_count, sdk_usage_count, last_error
            FROM software_global_status
            WHERE source_scope = ?1
            ",
            params![source_scope],
            |row| {
                Ok(SoftwareGlobalStatus {
                    repository_id: row.get(0)?,
                    source_scope: row.get(1)?,
                    projected_graph_version: GraphVersion::new(row.get::<_, u64>(2)?),
                    stale: row.get::<_, i64>(3)? != 0,
                    component_count: row.get(4)?,
                    sdk_usage_count: row.get(5)?,
                    last_error: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}

fn components_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareComponent>, StorageError> {
    let path_filter = path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter =
        language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let query = format!(
        "
        SELECT component_id, repository_id, source_scope, ecosystem, name, requirement,
               resolved_version, dependency_group, source_kind, relationship_state,
               language_id, evidence_path, evidence_line_start, evidence_line_end,
               confidence_basis_points, created_graph_version
        FROM software_components
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY ecosystem ASC, name ASC, relationship_state DESC, evidence_path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), component_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn sdk_usages_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    limit: usize,
) -> Result<Vec<SoftwareSdkUsage>, StorageError> {
    let path_filter = path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
    let language_filter =
        language_filter_sql_for_column("language_id", &request.repository.language_filters);
    let query = format!(
        "
        SELECT usage_id, repository_id, source_scope, language_id, module, target_hint,
               resolution_state, evidence_path, evidence_line_start, evidence_line_end,
               confidence_basis_points, created_graph_version
        FROM software_sdk_usages
        WHERE source_scope = ?1
        {path_filter}
        {language_filter}
        ORDER BY language_id ASC, module ASC, evidence_path ASC
        LIMIT ?
        ",
    );
    let mut values = vec![Value::Text(source_scope.to_owned())];
    push_path_filter_values(&mut values, &request.repository.path_filters);
    push_language_filter_values(&mut values, &request.repository.language_filters);
    values.push(Value::Integer(limit as i64));
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map(params_from_iter(values), sdk_usage_from_row)?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

fn component_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareComponent> {
    Ok(SoftwareComponent {
        component_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        ecosystem: row.get(3)?,
        name: row.get(4)?,
        requirement: row.get(5)?,
        resolved_version: row.get(6)?,
        dependency_group: row.get(7)?,
        source_kind: row.get(8)?,
        relationship_state: row.get(9)?,
        language_id: row.get(10)?,
        evidence_path: row.get(11)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(12)?,
            end: row.get(13)?,
        },
        confidence_basis_points: row.get(14)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(15)?),
    })
}

fn sdk_usage_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SoftwareSdkUsage> {
    Ok(SoftwareSdkUsage {
        usage_id: row.get(0)?,
        repository_id: row.get(1)?,
        source_scope: row.get(2)?,
        language_id: row.get(3)?,
        module: row.get(4)?,
        target_hint: row.get(5)?,
        resolution_state: row.get(6)?,
        evidence_path: row.get(7)?,
        evidence_line_range: RepositoryCodeRange {
            start: row.get(8)?,
            end: row.get(9)?,
        },
        confidence_basis_points: row.get(10)?,
        created_graph_version: GraphVersion::new(row.get::<_, u64>(11)?),
    })
}

#[cfg(test)]
#[path = "software_projection_tests.rs"]
mod software_projection_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_projection_materializes_dependencies_and_unresolved_imports() {
        let mut connection = Connection::open_in_memory().expect("sqlite should open");
        create_test_schema(&connection);
        initialize_schema(&connection).expect("software schema should initialize");
        seed_scope(&connection);

        let projection =
            refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

        assert_eq!(projection.status.component_count, 3);
        assert_eq!(projection.status.sdk_usage_count, 2);
        assert!(projection.components.iter().any(
            |component| component.name == "serde" && component.relationship_state == "declared"
        ));
        assert!(
            projection
                .components
                .iter()
                .any(|component| component.name == "serde"
                    && component.relationship_state == "locked")
        );
        assert_eq!(
            projection
                .components
                .iter()
                .filter(|component| component.name == "serde"
                    && component.relationship_state == "declared")
                .count(),
            2
        );
        assert_eq!(
            projection.sdk_usages[0].target_hint.as_deref(),
            Some("securec.h")
        );
    }

    #[test]
    fn projection_query_filters_kind_without_unrelated_graph_staleness() {
        let mut connection = Connection::open_in_memory().expect("sqlite should open");
        create_test_schema(&connection);
        initialize_schema(&connection).expect("software schema should initialize");
        seed_scope(&connection);
        refresh_projection(&mut connection, "scope-1").expect("projection should refresh");
        connection
            .execute("UPDATE graph_state SET graph_version = 2 WHERE id = 1", [])
            .expect("graph version should update");

        let request = SoftwareGlobalRequest::new(
            crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
                .expect("selector"),
            SoftwareGlobalKind::Sdks,
            crate::domain::FreshnessPolicy::AllowStale,
            10,
        )
        .expect("request should validate");
        let projection = projection(&mut connection, request).expect("projection should load");

        assert!(!projection.status.stale);
        assert!(projection.components.is_empty());
        assert_eq!(projection.sdk_usages.len(), 2);
    }

    #[test]
    fn projection_all_kind_keeps_combined_results_within_limit() {
        let mut connection = Connection::open_in_memory().expect("sqlite should open");
        create_test_schema(&connection);
        initialize_schema(&connection).expect("software schema should initialize");
        seed_scope(&connection);
        refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

        let request = SoftwareGlobalRequest::new(
            crate::domain::CodeRepositorySelector::new("repo", "commit-1", Vec::new(), Vec::new())
                .expect("selector"),
            SoftwareGlobalKind::All,
            crate::domain::FreshnessPolicy::AllowStale,
            4,
        )
        .expect("request should validate");
        let projection = projection(&mut connection, request).expect("projection should load");

        assert_eq!(projection.components.len() + projection.sdk_usages.len(), 4);
        assert_eq!(projection.components.len(), 3);
        assert_eq!(projection.sdk_usages.len(), 1);
    }

    #[test]
    fn projection_query_rejects_unindexed_refs() {
        let mut connection = Connection::open_in_memory().expect("sqlite should open");
        create_test_schema(&connection);
        initialize_schema(&connection).expect("software schema should initialize");
        seed_scope(&connection);
        refresh_projection(&mut connection, "scope-1").expect("projection should refresh");

        let missing_ref = SoftwareGlobalRequest::new(
            crate::domain::CodeRepositorySelector::new(
                "repo",
                "missing-commit",
                Vec::new(),
                Vec::new(),
            )
            .expect("selector"),
            SoftwareGlobalKind::All,
            crate::domain::FreshnessPolicy::AllowStale,
            10,
        )
        .expect("request should validate");
        let missing_ref_error =
            projection(&mut connection, missing_ref).expect_err("missing ref should fail");
        assert!(
            missing_ref_error
                .to_string()
                .contains("does not have an indexed software projection scope")
        );
    }

    fn create_test_schema(connection: &Connection) {
        connection
            .execute_batch(
                "
                CREATE TABLE graph_state (id INTEGER PRIMARY KEY CHECK (id = 1), graph_version INTEGER NOT NULL);
                INSERT INTO graph_state (id, graph_version) VALUES (1, 1);
                CREATE TABLE code_repository_scopes (
                    source_scope TEXT PRIMARY KEY,
                    repository_id TEXT NOT NULL,
                    resolved_commit_sha TEXT NOT NULL,
                    path_filters_json TEXT NOT NULL,
                    language_filters_json TEXT NOT NULL
                );
                CREATE TABLE code_repositories (
                    repository_id TEXT PRIMARY KEY,
                    alias TEXT NOT NULL,
                    last_indexed_scope_id TEXT
                );
                CREATE TABLE code_repository_aliases (
                    alias TEXT PRIMARY KEY,
                    repository_id TEXT NOT NULL
                );
                CREATE TABLE code_repository_dependencies (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    ecosystem TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    requirement TEXT,
                    resolved_version TEXT,
                    dependency_group TEXT NOT NULL,
                    source_kind TEXT NOT NULL,
                    is_lockfile INTEGER NOT NULL,
                    language_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                CREATE TABLE code_repository_files (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    file_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    language_id TEXT NOT NULL
                );
                CREATE TABLE code_repository_imports (
                    repository_id TEXT NOT NULL,
                    source_scope TEXT NOT NULL,
                    file_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    module TEXT NOT NULL,
                    target_hint TEXT,
                    resolution_state TEXT NOT NULL,
                    confidence_basis_points INTEGER NOT NULL,
                    line_start INTEGER NOT NULL,
                    line_end INTEGER NOT NULL
                );
                ",
            )
            .expect("test schema should initialize");
    }

    fn seed_scope(connection: &Connection) {
        connection
            .execute(
                "INSERT INTO code_repository_scopes (
                    source_scope, repository_id, resolved_commit_sha,
                    path_filters_json, language_filters_json
                ) VALUES ('scope-1', 'repo', 'commit-1', '[]', '[]')",
                [],
            )
            .expect("scope should insert");
        connection
            .execute(
                "INSERT INTO code_repositories (repository_id, alias, last_indexed_scope_id) VALUES ('repo', 'core', 'scope-1')",
                [],
            )
            .expect("repo should insert");
        connection
            .execute(
                "INSERT INTO code_repository_aliases (alias, repository_id) VALUES ('core', 'repo')",
                [],
            )
            .expect("alias should insert");
        connection
            .execute(
                "INSERT INTO code_repository_dependencies (
                    repository_id, source_scope, ecosystem, package_name, requirement,
                    resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                    path, line_start, line_end
                ) VALUES ('repo', 'scope-1', 'cargo', 'serde', '1', NULL, 'normal', 'manifest', 0, 'rust', 'Cargo.toml', 7, 7)",
                [],
            )
            .expect("manifest dependency should insert");
        connection
            .execute(
                "INSERT INTO code_repository_dependencies (
                    repository_id, source_scope, ecosystem, package_name, requirement,
                    resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                    path, line_start, line_end
                ) VALUES ('repo', 'scope-1', 'cargo', 'serde', '1', NULL, 'normal', 'manifest', 0, 'rust', 'crates/core/Cargo.toml', 9, 9)",
                [],
            )
            .expect("duplicate manifest dependency should insert");
        connection
            .execute(
                "INSERT INTO code_repository_dependencies (
                    repository_id, source_scope, ecosystem, package_name, requirement,
                    resolved_version, dependency_group, source_kind, is_lockfile, language_id,
                    path, line_start, line_end
                ) VALUES ('repo', 'scope-1', 'cargo', 'serde', NULL, '1.0.0', 'normal', 'lockfile', 1, 'rust', 'Cargo.lock', 33, 33)",
                [],
            )
            .expect("lock dependency should insert");
        connection
            .execute(
                "INSERT INTO code_repository_files (repository_id, source_scope, file_id, path, language_id) VALUES ('repo', 'scope-1', 'file-1', 'src/main.cc', 'cpp')",
                [],
            )
            .expect("file should insert");
        connection
            .execute(
                "INSERT INTO code_repository_imports (
                    repository_id, source_scope, file_id, path, module, target_hint,
                    resolution_state, confidence_basis_points, line_start, line_end
                ) VALUES ('repo', 'scope-1', 'file-1', 'src/main.cc', '#include <securec.h>', 'securec.h', 'unresolved', 2500, 3, 3)",
                [],
            )
            .expect("import should insert");
        connection
            .execute(
                "INSERT INTO code_repository_imports (
                    repository_id, source_scope, file_id, path, module, target_hint,
                    resolution_state, confidence_basis_points, line_start, line_end
                ) VALUES ('repo', 'scope-1', 'file-1', 'src/main.cc', '#include <securec.h>', 'securec.h', 'unresolved', 2500, 9, 9)",
                [],
            )
            .expect("repeated import should insert");
    }
}
