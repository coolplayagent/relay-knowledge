use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SoftwareComponent, SoftwareComponentInput,
        SoftwareGlobalKind, SoftwareGlobalProjection, SoftwareGlobalRequest, SoftwareGlobalStatus,
        SoftwareSdkUsage, SoftwareSdkUsageInput,
    },
    storage::StorageError,
};

use super::{super::current_graph_version, code_query_scope};

const SOFTWARE_PROJECTION_SCHEMA_VERSION: i64 = 2;

#[path = "software/dependency_usage.rs"]
mod dependency_usage;
#[path = "software_graph.rs"]
mod software_graph;
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

        CREATE TABLE IF NOT EXISTS software_files (
            software_file_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            path TEXT NOT NULL,
            language_id TEXT NOT NULL,
            file_role TEXT NOT NULL,
            parse_status TEXT NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_files_scope
            ON software_files(source_scope, file_role, path);

        CREATE INDEX IF NOT EXISTS software_files_scope_path
            ON software_files(source_scope, path);

        CREATE TABLE IF NOT EXISTS software_topics (
            topic_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            name TEXT NOT NULL,
            topic_kind TEXT NOT NULL,
            source_path TEXT NOT NULL,
            line_start INTEGER NOT NULL,
            line_end INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_topics_scope
            ON software_topics(source_scope, topic_kind, source_path);

        CREATE TABLE IF NOT EXISTS software_relationships (
            relationship_id TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            source_scope TEXT NOT NULL,
            relationship_kind TEXT NOT NULL,
            source_id TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            target_id TEXT NOT NULL,
            target_kind TEXT NOT NULL,
            target_hint TEXT,
            resolution_state TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            confidence_tier TEXT NOT NULL,
            evidence_path TEXT NOT NULL,
            evidence_line_start INTEGER NOT NULL,
            evidence_line_end INTEGER NOT NULL,
            created_graph_version INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS software_relationships_scope
            ON software_relationships(source_scope, relationship_kind, evidence_path);

        CREATE TABLE IF NOT EXISTS software_global_status (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            projected_graph_version INTEGER NOT NULL,
            stale INTEGER NOT NULL,
            component_count INTEGER NOT NULL,
            sdk_usage_count INTEGER NOT NULL,
            file_count INTEGER NOT NULL DEFAULT 0,
            topic_count INTEGER NOT NULL DEFAULT 0,
            relationship_count INTEGER NOT NULL DEFAULT 0,
            projection_schema_version INTEGER NOT NULL DEFAULT 2,
            last_error TEXT
        );
        ",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "software_global_status",
        "file_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "software_global_status",
        "topic_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "software_global_status",
        "relationship_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    super::super::schema_columns::ensure_column(
        connection,
        "software_global_status",
        "projection_schema_version",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    mark_legacy_projection_schema_stale(connection)?;

    dependency_usage::initialize_schema(connection)
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
    transaction.execute(
        "DELETE FROM software_files WHERE source_scope = ?1",
        params![source_scope],
    )?;
    transaction.execute(
        "DELETE FROM software_topics WHERE source_scope = ?1",
        params![source_scope],
    )?;
    transaction.execute(
        "DELETE FROM software_relationships WHERE source_scope = ?1",
        params![source_scope],
    )?;
    dependency_usage::delete_scope(&transaction, source_scope)?;

    let components = dependency_components(&transaction, source_scope, graph_version)?;
    for component in &components {
        insert_component(&transaction, component)?;
    }

    let dependency_usages = dependency_usage::derive_dependency_usages(
        &transaction,
        source_scope,
        graph_version,
        &components,
    )?;
    for usage in &dependency_usages {
        dependency_usage::insert_usage(&transaction, usage)?;
    }

    let sdk_usages = unresolved_sdk_usages(&transaction, source_scope, graph_version)?;
    for usage in &sdk_usages {
        insert_sdk_usage(&transaction, usage)?;
    }

    let file_count = software_graph::materialize_files(&transaction, source_scope, graph_version)?;

    let topic_count =
        software_graph::materialize_topics(&transaction, source_scope, graph_version)?;

    let relationship_count =
        software_graph::materialize_relationships(&transaction, source_scope, graph_version)?;

    let repository_id = repository_id_for_scope(&transaction, source_scope)?
        .unwrap_or_else(|| "unknown".to_owned());
    let status = SoftwareGlobalStatus {
        repository_id,
        source_scope: source_scope.to_owned(),
        projected_graph_version: graph_version,
        stale: false,
        component_count: components.len(),
        sdk_usage_count: sdk_usages.len(),
        file_count,
        topic_count,
        relationship_count,
        last_error: None,
    };
    upsert_status(&transaction, &status)?;
    transaction.commit()?;

    Ok(SoftwareGlobalProjection {
        status,
        components,
        dependency_usages,
        sdk_usages,
        files: Vec::new(),
        topics: Vec::new(),
        relationships: Vec::new(),
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
            file_count: 0,
            topic_count: 0,
            relationship_count: 0,
            last_error: Some("software global projection has not been refreshed".to_owned()),
        });
    let components = match request.kind {
        SoftwareGlobalKind::Dependencies | SoftwareGlobalKind::All => {
            components_for_scope(connection, source_scope, &request, request.limit)?
        }
        SoftwareGlobalKind::Sdks
        | SoftwareGlobalKind::Files
        | SoftwareGlobalKind::Topics
        | SoftwareGlobalKind::Relationships => Vec::new(),
    };
    let dependency_usages = match request.kind {
        SoftwareGlobalKind::Dependencies => {
            let remaining = request.limit.saturating_sub(components.len());
            dependency_usage::usages_for_scope(connection, source_scope, &request, remaining)?
        }
        SoftwareGlobalKind::All => {
            let remaining = request.limit.saturating_sub(components.len());
            if remaining == 0 {
                Vec::new()
            } else {
                dependency_usage::usages_for_scope(connection, source_scope, &request, remaining)?
            }
        }
        SoftwareGlobalKind::Sdks
        | SoftwareGlobalKind::Files
        | SoftwareGlobalKind::Topics
        | SoftwareGlobalKind::Relationships => Vec::new(),
    };
    let sdk_usages = match request.kind {
        SoftwareGlobalKind::Sdks => {
            sdk_usages_for_scope(connection, source_scope, &request, request.limit)?
        }
        SoftwareGlobalKind::All => {
            let remaining = request
                .limit
                .saturating_sub(components.len())
                .saturating_sub(dependency_usages.len());
            if remaining == 0 {
                Vec::new()
            } else {
                sdk_usages_for_scope(connection, source_scope, &request, remaining)?
            }
        }
        SoftwareGlobalKind::Dependencies
        | SoftwareGlobalKind::Files
        | SoftwareGlobalKind::Topics
        | SoftwareGlobalKind::Relationships => Vec::new(),
    };
    let files = match request.kind {
        SoftwareGlobalKind::Files => {
            software_graph::files_for_scope(connection, source_scope, &request, request.limit)?
        }
        SoftwareGlobalKind::All => {
            let remaining = request
                .limit
                .saturating_sub(components.len())
                .saturating_sub(dependency_usages.len())
                .saturating_sub(sdk_usages.len());
            if remaining == 0 {
                Vec::new()
            } else {
                software_graph::files_for_scope(connection, source_scope, &request, remaining)?
            }
        }
        SoftwareGlobalKind::Dependencies
        | SoftwareGlobalKind::Sdks
        | SoftwareGlobalKind::Topics
        | SoftwareGlobalKind::Relationships => Vec::new(),
    };
    let topics = match request.kind {
        SoftwareGlobalKind::Topics => {
            software_graph::topics_for_scope(connection, source_scope, &request, request.limit)?
        }
        SoftwareGlobalKind::All => {
            let remaining = request
                .limit
                .saturating_sub(components.len())
                .saturating_sub(dependency_usages.len())
                .saturating_sub(sdk_usages.len())
                .saturating_sub(files.len());
            if remaining == 0 {
                Vec::new()
            } else {
                software_graph::topics_for_scope(connection, source_scope, &request, remaining)?
            }
        }
        SoftwareGlobalKind::Dependencies
        | SoftwareGlobalKind::Sdks
        | SoftwareGlobalKind::Files
        | SoftwareGlobalKind::Relationships => Vec::new(),
    };
    let relationships = match request.kind {
        SoftwareGlobalKind::Relationships => software_graph::relationships_for_scope(
            connection,
            source_scope,
            &request,
            request.limit,
        )?,
        SoftwareGlobalKind::All => {
            let remaining = request
                .limit
                .saturating_sub(components.len())
                .saturating_sub(dependency_usages.len())
                .saturating_sub(sdk_usages.len())
                .saturating_sub(files.len())
                .saturating_sub(topics.len());
            if remaining == 0 {
                Vec::new()
            } else {
                software_graph::relationships_for_scope(
                    connection,
                    source_scope,
                    &request,
                    remaining,
                )?
            }
        }
        SoftwareGlobalKind::Dependencies
        | SoftwareGlobalKind::Sdks
        | SoftwareGlobalKind::Files
        | SoftwareGlobalKind::Topics => Vec::new(),
    };

    Ok(SoftwareGlobalProjection {
        status,
        components,
        dependency_usages,
        sdk_usages,
        files,
        topics,
        relationships,
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
            component_count, sdk_usage_count, file_count, topic_count,
            relationship_count, projection_schema_version, last_error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            projected_graph_version = excluded.projected_graph_version,
            stale = excluded.stale,
            component_count = excluded.component_count,
            sdk_usage_count = excluded.sdk_usage_count,
            file_count = excluded.file_count,
            topic_count = excluded.topic_count,
            relationship_count = excluded.relationship_count,
            projection_schema_version = excluded.projection_schema_version,
            last_error = excluded.last_error
        ",
        params![
            status.source_scope,
            status.repository_id,
            status.projected_graph_version.get(),
            if status.stale { 1_i64 } else { 0_i64 },
            status.component_count,
            status.sdk_usage_count,
            status.file_count,
            status.topic_count,
            status.relationship_count,
            SOFTWARE_PROJECTION_SCHEMA_VERSION,
            status.last_error,
        ],
    )?;

    Ok(())
}

fn mark_legacy_projection_schema_stale(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE software_global_status
        SET stale = 1,
            projection_schema_version = ?1,
            last_error = COALESCE(
                last_error,
                'software global projection schema changed; refresh required'
            )
        WHERE projection_schema_version < ?1
        ",
        params![SOFTWARE_PROJECTION_SCHEMA_VERSION],
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
                   component_count, sdk_usage_count, file_count, topic_count,
                   relationship_count, last_error
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
                    file_count: row.get(6)?,
                    topic_count: row.get(7)?,
                    relationship_count: row.get(8)?,
                    last_error: row.get(9)?,
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
#[path = "software_tests.rs"]
mod tests;
