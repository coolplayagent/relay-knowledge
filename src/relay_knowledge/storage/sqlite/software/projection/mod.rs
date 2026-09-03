use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value};

use crate::{
    domain::{
        GraphVersion, RepositoryCodeRange, SOFTWARE_ONTOLOGY_VERSION, SoftwareBuildTarget,
        SoftwareComponent, SoftwareComponentInput, SoftwareDependencyUsage, SoftwareDesignElement,
        SoftwareEntity, SoftwareFile, SoftwareGlobalKind, SoftwareGlobalProjection,
        SoftwareGlobalRequest, SoftwareGlobalStatus, SoftwareIacResource,
        SoftwareProjectionFreshness, SoftwareRelationship, SoftwareSdkUsage, SoftwareSdkUsageInput,
        SoftwareShapeDiagnostic, SoftwareSourceCoverage, SoftwareStatement, SoftwareTopic,
    },
    storage::StorageError,
};

use super::super::graph::current_graph_version;
use super::{
    dependency_usage, graph, lifecycle,
    query_scope::{
        language_filter_sql_for_column, path_filter_sql_for_column, push_language_filter_values,
        push_path_filter_values, repository_id_for_scope, source_scope_for_request,
    },
    schema::SOFTWARE_PROJECTION_SCHEMA_VERSION,
};

mod component_order;
mod entity_targets;
mod fair_limit;
mod fenced;

pub(in super::super) use fenced::{
    FencedProjectionAdvance, advance_fenced_projection, refreshed_fenced_projection,
};

const MAX_DEPENDENCY_COMPONENTS_PER_SCOPE: usize = 65_536;
const MAX_SDK_USAGES_PER_SCOPE: usize = 131_072;
const COMPONENT_USAGE_TARGET_QUERY_BATCH_SIZE: usize = 256;

#[derive(Default)]
struct ProjectionSlices {
    components: Vec<SoftwareComponent>,
    dependency_usages: Vec<SoftwareDependencyUsage>,
    sdk_usages: Vec<SoftwareSdkUsage>,
    files: Vec<SoftwareFile>,
    topics: Vec<SoftwareTopic>,
    relationships: Vec<SoftwareRelationship>,
    build_targets: Vec<SoftwareBuildTarget>,
    iac_resources: Vec<SoftwareIacResource>,
    design_elements: Vec<SoftwareDesignElement>,
    entities: Vec<SoftwareEntity>,
    statements: Vec<SoftwareStatement>,
    diagnostics: Vec<SoftwareShapeDiagnostic>,
}

pub(in super::super) fn refresh_projection(
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
    lifecycle::delete_scope(&transaction, source_scope)?;
    super::ontology::delete_scope(&transaction, source_scope)?;

    let components = dependency_components(&transaction, source_scope, graph_version)?;
    insert_components(&transaction, &components)?;

    let dependency_usages = dependency_usage::derive_dependency_usages(
        &transaction,
        source_scope,
        graph_version,
        &components,
    )?;
    dependency_usage::insert_usages(&transaction, &dependency_usages)?;

    let sdk_usages = unresolved_sdk_usages(&transaction, source_scope, graph_version)?;
    insert_sdk_usages(&transaction, &sdk_usages)?;
    let lifecycle_projection =
        lifecycle::refresh_projection(&transaction, source_scope, graph_version)?;

    let file_count = graph::materialize_files(&transaction, source_scope, graph_version)?;

    let topic_count = graph::materialize_topics(&transaction, source_scope, graph_version)?;

    let relationship_count =
        graph::materialize_relationships(&transaction, source_scope, graph_version)?;
    let ontology_projection =
        super::ontology::refresh_projection(&transaction, source_scope, graph_version)?;

    let repository_id = repository_id_for_scope(&transaction, source_scope)?
        .unwrap_or_else(|| "unknown".to_owned());
    let status = SoftwareGlobalStatus {
        repository_id,
        source_scope: source_scope.to_owned(),
        projected_graph_version: graph_version,
        stale: false,
        ontology_version: SOFTWARE_ONTOLOGY_VERSION.to_owned(),
        projection_schema_version: SOFTWARE_PROJECTION_SCHEMA_VERSION as u32,
        source_coverage: ontology_projection.source_coverage.clone(),
        completeness_basis_points: ontology_projection.completeness_basis_points,
        freshness: SoftwareProjectionFreshness::Fresh,
        conflict_count: ontology_projection.conflict_count,
        entity_count: ontology_projection.entities.len(),
        statement_count: ontology_projection.statements.len(),
        diagnostic_count: ontology_projection.diagnostics.len(),
        component_count: components.len(),
        sdk_usage_count: sdk_usages.len(),
        file_count,
        topic_count,
        relationship_count,
        build_target_count: lifecycle_projection.build_targets.len(),
        iac_resource_count: lifecycle_projection.iac_resources.len(),
        design_element_count: lifecycle_projection.design_elements.len(),
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
        build_targets: lifecycle_projection.build_targets,
        iac_resources: lifecycle_projection.iac_resources,
        design_elements: lifecycle_projection.design_elements,
        entities: ontology_projection.entities,
        statements: ontology_projection.statements,
        diagnostics: ontology_projection.diagnostics,
    })
}

pub(in super::super) fn projection(
    connection: &mut Connection,
    request: SoftwareGlobalRequest,
) -> Result<SoftwareGlobalProjection, StorageError> {
    let source_scope = source_scope_for_request(connection, &request)?;
    projection_for_scope(connection, &source_scope, request)
}

pub(in super::super) fn projection_for_scope(
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
            ontology_version: SOFTWARE_ONTOLOGY_VERSION.to_owned(),
            projection_schema_version: SOFTWARE_PROJECTION_SCHEMA_VERSION as u32,
            source_coverage: SoftwareSourceCoverage::default(),
            completeness_basis_points: 0,
            freshness: SoftwareProjectionFreshness::Stale,
            conflict_count: 0,
            entity_count: 0,
            statement_count: 0,
            diagnostic_count: 0,
            component_count: 0,
            sdk_usage_count: 0,
            file_count: 0,
            topic_count: 0,
            relationship_count: 0,
            build_target_count: 0,
            iac_resource_count: 0,
            design_element_count: 0,
            last_error: Some("software global projection has not been refreshed".to_owned()),
        });
    let slices = projection_slices(connection, source_scope, &request)?;

    Ok(SoftwareGlobalProjection {
        status,
        components: slices.components,
        dependency_usages: slices.dependency_usages,
        sdk_usages: slices.sdk_usages,
        files: slices.files,
        topics: slices.topics,
        relationships: slices.relationships,
        build_targets: slices.build_targets,
        iac_resources: slices.iac_resources,
        design_elements: slices.design_elements,
        entities: slices.entities,
        statements: slices.statements,
        diagnostics: slices.diagnostics,
    })
}

fn projection_slices(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
) -> Result<ProjectionSlices, StorageError> {
    match request.kind {
        SoftwareGlobalKind::Dependencies => {
            let components =
                components_for_scope(connection, source_scope, request, request.limit)?;
            let remaining = request.limit.saturating_sub(components.len());
            let dependency_usages =
                dependency_usage::usages_for_scope(connection, source_scope, request, remaining)?;
            Ok(ProjectionSlices {
                components,
                dependency_usages,
                ..ProjectionSlices::default()
            })
        }
        SoftwareGlobalKind::Sdks => Ok(ProjectionSlices {
            sdk_usages: sdk_usages_for_scope(connection, source_scope, request, request.limit)?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Files => Ok(ProjectionSlices {
            files: graph::files_for_scope(connection, source_scope, request, request.limit)?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Topics => Ok(ProjectionSlices {
            topics: graph::topics_for_scope(connection, source_scope, request, request.limit)?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Relationships => Ok(ProjectionSlices {
            relationships: graph::relationships_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Build => Ok(ProjectionSlices {
            build_targets: lifecycle::build_targets_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Iac => Ok(ProjectionSlices {
            iac_resources: lifecycle::iac_resources_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Design => Ok(ProjectionSlices {
            design_elements: lifecycle::design_elements_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Systems
        | SoftwareGlobalKind::Apis
        | SoftwareGlobalKind::Resources
        | SoftwareGlobalKind::Tests
        | SoftwareGlobalKind::Deployments
        | SoftwareGlobalKind::Releases => Ok(ProjectionSlices {
            entities: super::ontology::entities_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Statements => Ok(ProjectionSlices {
            entities: super::ontology::entities_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?,
            statements: super::ontology::statements_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?,
            ..ProjectionSlices::default()
        }),
        SoftwareGlobalKind::Conflicts => {
            let statements = super::ontology::statements_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?;
            let remaining = request.limit.saturating_sub(statements.len());
            Ok(ProjectionSlices {
                statements,
                diagnostics: super::ontology::diagnostics_for_scope(
                    connection,
                    source_scope,
                    remaining,
                )?,
                ..ProjectionSlices::default()
            })
        }
        SoftwareGlobalKind::All => {
            let mut components =
                components_for_scope(connection, source_scope, request, request.limit)?;
            let dependency_usages = dependency_usage::usages_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?;
            add_usage_target_components(
                connection,
                source_scope,
                request,
                &mut components,
                &dependency_usages,
            )?;
            let mut entities = super::ontology::entities_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?;
            let statements = super::ontology::statements_for_scope(
                connection,
                source_scope,
                request,
                request.limit,
            )?;
            entity_targets::append_statement_targets(
                connection,
                source_scope,
                request,
                &mut entities,
                &statements,
            )?;
            let mut slices = ProjectionSlices {
                components,
                dependency_usages,
                sdk_usages: sdk_usages_for_scope(connection, source_scope, request, request.limit)?,
                files: graph::files_for_scope(connection, source_scope, request, request.limit)?,
                topics: graph::topics_for_scope(connection, source_scope, request, request.limit)?,
                relationships: graph::relationships_for_scope(
                    connection,
                    source_scope,
                    request,
                    request.limit,
                )?,
                build_targets: lifecycle::build_targets_for_scope(
                    connection,
                    source_scope,
                    request,
                    request.limit,
                )?,
                iac_resources: lifecycle::iac_resources_for_scope(
                    connection,
                    source_scope,
                    request,
                    request.limit,
                )?,
                design_elements: lifecycle::design_elements_for_scope(
                    connection,
                    source_scope,
                    request,
                    request.limit,
                )?,
                entities,
                statements,
                diagnostics: super::ontology::diagnostics_for_scope(
                    connection,
                    source_scope,
                    request.limit,
                )?,
            };
            fair_limit::apply_fair_total_limit(&mut slices, request.limit);
            Ok(slices)
        }
    }
}

fn dependency_components(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<Vec<SoftwareComponent>, StorageError> {
    dependency_components_with_limit(
        connection,
        source_scope,
        graph_version,
        MAX_DEPENDENCY_COMPONENTS_PER_SCOPE,
    )
}

fn dependency_components_with_limit(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
) -> Result<Vec<SoftwareComponent>, StorageError> {
    let mut statement = connection.prepare(
        "
        WITH ranked_dependencies AS (
            SELECT repository_id, source_scope, ecosystem, package_name, requirement,
                   resolved_version, dependency_group, source_kind, is_lockfile,
                   language_id, path, line_start, line_end,
                   ROW_NUMBER() OVER (
                       PARTITION BY
                           CASE WHEN is_lockfile = 0 THEN 0 ELSE 1 END,
                           repository_id, source_scope, ecosystem, package_name,
                           requirement, resolved_version, dependency_group,
                           source_kind, language_id
                       ORDER BY path ASC, line_start ASC, line_end ASC
                   ) AS evidence_rank
            FROM code_repository_dependencies
            WHERE source_scope = ?1
        )
        SELECT repository_id, source_scope, ecosystem, package_name, requirement,
               resolved_version, dependency_group, source_kind, is_lockfile,
               language_id, path, line_start, line_end
        FROM ranked_dependencies
        WHERE is_lockfile = 0 OR evidence_rank = 1
        ORDER BY ecosystem ASC, package_name ASC, is_lockfile DESC, path ASC,
                 line_start ASC, line_end ASC, resolved_version ASC, requirement ASC,
                 dependency_group ASC, source_kind ASC, language_id ASC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(
        params![source_scope, limit.saturating_add(1) as i64],
        |row| {
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
        },
    )?;

    let components = rows
        .map(|row| {
            row.map_err(StorageError::from).and_then(|input| {
                SoftwareComponent::new(input)
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if components.len() > limit {
        return Err(StorageError::CapacityExceeded(format!(
            "software dependency components exceed the bounded limit {limit}"
        )));
    }
    Ok(components)
}

fn unresolved_sdk_usages(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
) -> Result<Vec<SoftwareSdkUsage>, StorageError> {
    unresolved_sdk_usages_with_limit(
        connection,
        source_scope,
        graph_version,
        MAX_SDK_USAGES_PER_SCOPE,
    )
}

fn unresolved_sdk_usages_with_limit(
    connection: &Connection,
    source_scope: &str,
    graph_version: GraphVersion,
    limit: usize,
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
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(
        params![source_scope, limit.saturating_add(1) as i64],
        |row| {
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
        },
    )?;

    let usages = rows
        .map(|row| {
            row.map_err(StorageError::from).and_then(|input| {
                SoftwareSdkUsage::new(input)
                    .map_err(|error| StorageError::InvalidInput(error.to_string()))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if usages.len() > limit {
        return Err(StorageError::CapacityExceeded(format!(
            "software SDK usages exceed the bounded limit {limit}"
        )));
    }
    Ok(usages)
}

fn insert_components(
    connection: &Connection,
    components: &[SoftwareComponent],
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        INSERT OR REPLACE INTO software_components (
            component_id, repository_id, source_scope, ecosystem, name, requirement,
            resolved_version, dependency_group, source_kind, relationship_state,
            language_id, evidence_path, evidence_line_start, evidence_line_end,
            confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        ",
    )?;
    for component in components {
        statement.execute(params![
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
        ])?;
    }

    Ok(())
}

fn insert_sdk_usages(
    connection: &Connection,
    usages: &[SoftwareSdkUsage],
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "
        INSERT OR REPLACE INTO software_sdk_usages (
            usage_id, repository_id, source_scope, language_id, module, target_hint,
            resolution_state, evidence_path, evidence_line_start, evidence_line_end,
            confidence_basis_points, created_graph_version
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        ",
    )?;
    for usage in usages {
        statement.execute(params![
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
        ])?;
    }

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
            relationship_count, build_target_count, iac_resource_count,
            design_element_count, projection_schema_version, ontology_version,
            source_coverage_json, completeness_basis_points, freshness,
            conflict_count, entity_count, statement_count, diagnostic_count, last_error
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
        ON CONFLICT(source_scope) DO UPDATE SET
            repository_id = excluded.repository_id,
            projected_graph_version = excluded.projected_graph_version,
            stale = excluded.stale,
            component_count = excluded.component_count,
            sdk_usage_count = excluded.sdk_usage_count,
            file_count = excluded.file_count,
            topic_count = excluded.topic_count,
            relationship_count = excluded.relationship_count,
            build_target_count = excluded.build_target_count,
            iac_resource_count = excluded.iac_resource_count,
            design_element_count = excluded.design_element_count,
            projection_schema_version = excluded.projection_schema_version,
            ontology_version = excluded.ontology_version,
            source_coverage_json = excluded.source_coverage_json,
            completeness_basis_points = excluded.completeness_basis_points,
            freshness = excluded.freshness,
            conflict_count = excluded.conflict_count,
            entity_count = excluded.entity_count,
            statement_count = excluded.statement_count,
            diagnostic_count = excluded.diagnostic_count,
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
            status.build_target_count,
            status.iac_resource_count,
            status.design_element_count,
            SOFTWARE_PROJECTION_SCHEMA_VERSION,
            status.ontology_version,
            serde_json::to_string(&status.source_coverage).map_err(|error| {
                StorageError::Invariant(format!(
                    "software source coverage cannot be serialized: {error}"
                ))
            })?,
            status.completeness_basis_points,
            status.freshness.as_str(),
            status.conflict_count,
            status.entity_count,
            status.statement_count,
            status.diagnostic_count,
            status.last_error,
        ],
    )?;

    Ok(())
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
                   relationship_count, build_target_count, iac_resource_count,
                   design_element_count, projection_schema_version, ontology_version,
                   source_coverage_json, completeness_basis_points, freshness,
                   conflict_count, entity_count, statement_count, diagnostic_count,
                   last_error
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
                    ontology_version: row.get(13)?,
                    projection_schema_version: row.get::<_, u32>(12)?,
                    source_coverage: serde_json::from_str(&row.get::<_, String>(14)?).map_err(
                        |error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                14,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        },
                    )?,
                    completeness_basis_points: row.get(15)?,
                    freshness: SoftwareProjectionFreshness::parse(&row.get::<_, String>(16)?)
                        .ok_or_else(|| {
                            rusqlite::Error::InvalidColumnType(
                                16,
                                "freshness".to_owned(),
                                rusqlite::types::Type::Text,
                            )
                        })?,
                    conflict_count: row.get(17)?,
                    entity_count: row.get(18)?,
                    statement_count: row.get(19)?,
                    diagnostic_count: row.get(20)?,
                    component_count: row.get(4)?,
                    sdk_usage_count: row.get(5)?,
                    file_count: row.get(6)?,
                    topic_count: row.get(7)?,
                    relationship_count: row.get(8)?,
                    build_target_count: row.get(9)?,
                    iac_resource_count: row.get(10)?,
                    design_element_count: row.get(11)?,
                    last_error: row.get(21)?,
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
        ORDER BY ecosystem ASC, name ASC, relationship_state DESC, evidence_path ASC,
                 component_id ASC
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

fn add_usage_target_components(
    connection: &Connection,
    source_scope: &str,
    request: &SoftwareGlobalRequest,
    components: &mut Vec<SoftwareComponent>,
    dependency_usages: &[SoftwareDependencyUsage],
) -> Result<(), StorageError> {
    let mut seen_ids = components
        .iter()
        .map(|component| component.component_id.clone())
        .collect::<BTreeSet<_>>();
    let target_ids = dependency_usages
        .iter()
        .filter_map(|usage| {
            seen_ids
                .insert(usage.component_id.clone())
                .then_some(usage.component_id.as_str())
        })
        .collect::<Vec<_>>();

    for batch in target_ids.chunks(COMPONENT_USAGE_TARGET_QUERY_BATCH_SIZE) {
        let placeholders = std::iter::repeat_n("?", batch.len())
            .collect::<Vec<_>>()
            .join(", ");
        let path_filter =
            path_filter_sql_for_column("evidence_path", &request.repository.path_filters);
        let language_filter =
            language_filter_sql_for_column("language_id", &request.repository.language_filters);
        let query = format!(
            "
            SELECT component_id, repository_id, source_scope, ecosystem, name, requirement,
                   resolved_version, dependency_group, source_kind, relationship_state,
                   language_id, evidence_path, evidence_line_start, evidence_line_end,
                   confidence_basis_points, created_graph_version
            FROM software_components
            WHERE source_scope = ?1 AND component_id IN ({placeholders})
            {path_filter}
            {language_filter}
            ORDER BY ecosystem ASC, name ASC, relationship_state DESC, evidence_path ASC,
                     component_id ASC
            "
        );
        let mut values = std::iter::once(Value::Text(source_scope.to_owned()))
            .chain(batch.iter().map(|id| Value::Text((*id).to_owned())))
            .collect::<Vec<_>>();
        push_path_filter_values(&mut values, &request.repository.path_filters);
        push_language_filter_values(&mut values, &request.repository.language_filters);
        let mut statement = connection.prepare(&query)?;
        let rows = statement.query_map(params_from_iter(values), component_from_row)?;
        components.extend(
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(StorageError::from)?,
        );
    }
    component_order::sort_by_canonical_evidence(components);
    Ok(())
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
#[path = "filter_tests.rs"]
mod filter_tests;

#[cfg(test)]
#[path = "dependency_projection_tests.rs"]
mod dependency_projection_tests;

#[cfg(test)]
#[path = "test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
