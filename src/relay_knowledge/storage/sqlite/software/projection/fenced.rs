//! Durable, lease-friendly phases for fenced software projection publication.

use rusqlite::{Connection, Transaction, params};

use crate::{
    domain::{
        CodeRepositorySelector, CodeSoftwareProjectionPhase, FreshnessPolicy, SoftwareGlobalKind,
        SoftwareGlobalProjection, SoftwareGlobalRequest, SoftwareGlobalStatus,
        code_software_projection_phase,
    },
    storage::StorageError,
};

use super::{
    components_for_scope, dependency_components, insert_components, insert_sdk_usages,
    sdk_usages_for_scope, status_for_scope, unresolved_sdk_usages, upsert_status,
};
use crate::storage::sqlite::{code, graph::current_graph_version_in_transaction};

pub(in crate::storage::sqlite) enum FencedProjectionAdvance {
    Pending { checkpoint_state: String },
    Complete,
}

/// Advances exactly one projection phase and commits its checkpoint in the same transaction.
pub(in crate::storage::sqlite) fn advance_fenced_projection(
    connection: &mut Connection,
    source_scope: &str,
    fence: &code::lifecycle::publication_fence::PublicationFenceGuard,
) -> Result<FencedProjectionAdvance, StorageError> {
    let transaction = connection.transaction()?;
    validate_fence(&transaction, source_scope, fence)?;
    let checkpoint_state = prepare_checkpoint(&transaction, source_scope, fence)?;
    if is_terminal_checkpoint(&checkpoint_state) {
        let projection_is_fresh =
            status_for_scope(&transaction, source_scope)?.is_some_and(|status| !status.stale);
        if projection_is_fresh {
            validate_fence(&transaction, source_scope, fence)?;
            transaction.commit()?;
            return Ok(FencedProjectionAdvance::Complete);
        }
        code::publication::advance_software_projection_checkpoint(
            &transaction,
            source_scope,
            &checkpoint_state,
            CodeSoftwareProjectionPhase::Reset.checkpoint_state(),
        )?;
    }
    let checkpoint_state = if is_terminal_checkpoint(&checkpoint_state) {
        CodeSoftwareProjectionPhase::Reset
            .checkpoint_state()
            .to_owned()
    } else {
        checkpoint_state
    };
    let phase = code_software_projection_phase(&checkpoint_state).ok_or_else(|| {
        StorageError::Invariant(format!(
            "code scope '{source_scope}' has unknown software projection checkpoint '{checkpoint_state}'"
        ))
    })?;

    match phase {
        CodeSoftwareProjectionPhase::Reset => reset(&transaction, source_scope)?,
        CodeSoftwareProjectionPhase::Dependencies => {
            materialize_dependencies(&transaction, source_scope)?
        }
        CodeSoftwareProjectionPhase::SdkUsages => {
            materialize_sdk_usages(&transaction, source_scope)?
        }
        CodeSoftwareProjectionPhase::Lifecycle => {
            materialize_lifecycle(&transaction, source_scope)?
        }
        CodeSoftwareProjectionPhase::Files => materialize_files(&transaction, source_scope)?,
        CodeSoftwareProjectionPhase::Topics => materialize_topics(&transaction, source_scope)?,
        CodeSoftwareProjectionPhase::Relationships => {
            materialize_relationships(&transaction, source_scope)?
        }
        CodeSoftwareProjectionPhase::Ontology => materialize_ontology(&transaction, source_scope)?,
        CodeSoftwareProjectionPhase::Publish => {
            require_staged_status(&transaction, source_scope)?;
            code::publication::complete_after_software_projection(
                &transaction,
                source_scope,
                fence,
            )?;
            validate_fence(&transaction, source_scope, fence)?;
            transaction.commit()?;
            return Ok(FencedProjectionAdvance::Complete);
        }
    }

    let next_state = phase.next().ok_or_else(|| {
        StorageError::Invariant("software projection publish phase did not complete".to_owned())
    })?;
    code::publication::advance_software_projection_checkpoint(
        &transaction,
        source_scope,
        &checkpoint_state,
        next_state.checkpoint_state(),
    )?;
    validate_fence(&transaction, source_scope, fence)?;
    transaction.commit()?;
    Ok(FencedProjectionAdvance::Pending {
        checkpoint_state: next_state.checkpoint_state().to_owned(),
    })
}

fn prepare_checkpoint(
    transaction: &Transaction<'_>,
    source_scope: &str,
    fence: &code::lifecycle::publication_fence::PublicationFenceGuard,
) -> Result<String, StorageError> {
    if let Some(state) =
        code::publication::software_projection_checkpoint_state(transaction, source_scope)?
    {
        return Ok(state);
    }
    let resource_budget_json = serde_json::to_string(&fence.resource_budget(transaction)?)
        .map_err(|error| {
            StorageError::Invariant(format!(
                "software projection resource budget cannot be serialized: {error}"
            ))
        })?;
    let updated_at_ms = crate::clock::system_now_millis()
        .map_err(|error| StorageError::Invariant(error.to_string()))?;
    let reset_state = CodeSoftwareProjectionPhase::Reset.checkpoint_state();
    let inserted = transaction.execute(
        "
        INSERT INTO code_repository_index_checkpoints (
            source_scope, repository_id, state, resolved_commit_sha, tree_hash,
            path_filters_json, language_filters_json, total_path_count,
            parsed_file_count, committed_file_count, committed_symbol_count,
            committed_reference_count, committed_chunk_count, committed_fact_row_count,
            incremental_summary_json, batch_count, last_path,
            resource_budget_json, updated_at_ms, error_message
        )
        SELECT source_scope, repository_id, ?2, resolved_commit_sha, tree_hash,
               path_filters_json, language_filters_json, indexed_file_count,
               indexed_file_count, indexed_file_count, symbol_count,
               reference_count, chunk_count, 0,
               NULL, 0, NULL, ?3, ?4, NULL
        FROM code_repository_scopes
        WHERE source_scope = ?1
        ",
        params![
            source_scope,
            reset_state,
            resource_budget_json,
            updated_at_ms
        ],
    )?;
    if inserted != 1 {
        return Err(StorageError::Invariant(format!(
            "software projection scope '{source_scope}' disappeared before checkpoint initialization"
        )));
    }
    Ok(reset_state.to_owned())
}

fn is_terminal_checkpoint(state: &str) -> bool {
    matches!(state, "completed" | "finalizing:partitioned_publish")
}

/// Rehydrates the same slices historically returned by the atomic refresh path.
pub(in crate::storage::sqlite) fn refreshed_fenced_projection(
    connection: &mut Connection,
    source_scope: &str,
) -> Result<SoftwareGlobalProjection, StorageError> {
    let status = status_for_scope(connection, source_scope)?.ok_or_else(|| {
        StorageError::Invariant(format!(
            "software projection status for scope '{source_scope}' disappeared after publication"
        ))
    })?;
    if status.stale {
        return Err(StorageError::Invariant(format!(
            "software projection for scope '{source_scope}' remained stale after publication"
        )));
    }
    let request = unfiltered_request(&status);
    let dependency_usage_count = count_dependency_usages(connection, source_scope)?;

    Ok(SoftwareGlobalProjection {
        components: components_for_scope(
            connection,
            source_scope,
            &request,
            status.component_count,
        )?,
        dependency_usages: super::super::dependency_usage::usages_for_scope(
            connection,
            source_scope,
            &request,
            dependency_usage_count,
        )?,
        sdk_usages: sdk_usages_for_scope(
            connection,
            source_scope,
            &request,
            status.sdk_usage_count,
        )?,
        files: Vec::new(),
        topics: Vec::new(),
        relationships: Vec::new(),
        build_targets: super::super::lifecycle::build_targets_for_scope(
            connection,
            source_scope,
            &request,
            status.build_target_count,
        )?,
        iac_resources: super::super::lifecycle::iac_resources_for_scope(
            connection,
            source_scope,
            &request,
            status.iac_resource_count,
        )?,
        design_elements: super::super::lifecycle::design_elements_for_scope(
            connection,
            source_scope,
            &request,
            status.design_element_count,
        )?,
        entities: super::super::ontology::entities_for_scope(
            connection,
            source_scope,
            &request,
            status.entity_count,
        )?,
        statements: super::super::ontology::statements_for_scope(
            connection,
            source_scope,
            &request,
            status.statement_count,
        )?,
        diagnostics: super::super::ontology::diagnostics_for_scope(
            connection,
            source_scope,
            status.diagnostic_count,
        )?,
        status,
    })
}

fn reset(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    connection.execute(
        "DELETE FROM software_components WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "DELETE FROM software_sdk_usages WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "DELETE FROM software_files WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "DELETE FROM software_topics WHERE source_scope = ?1",
        params![source_scope],
    )?;
    connection.execute(
        "DELETE FROM software_relationships WHERE source_scope = ?1",
        params![source_scope],
    )?;
    super::super::dependency_usage::delete_scope(connection, source_scope)?;
    super::super::lifecycle::delete_scope(connection, source_scope)?;
    super::super::ontology::delete_scope(connection, source_scope)?;
    let repository_id =
        super::super::query_scope::repository_id_for_scope(connection, source_scope)?.ok_or_else(
            || {
                StorageError::Invariant(format!(
                    "repository identity for software projection scope '{source_scope}' is missing"
                ))
            },
        )?;
    upsert_status(
        connection,
        &SoftwareGlobalStatus {
            repository_id,
            source_scope: source_scope.to_owned(),
            projected_graph_version: current_graph_version_in_transaction(connection)?,
            stale: true,
            ontology_version: crate::domain::SOFTWARE_ONTOLOGY_VERSION.to_owned(),
            projection_schema_version: super::SOFTWARE_PROJECTION_SCHEMA_VERSION as u32,
            source_coverage: crate::domain::SoftwareSourceCoverage::default(),
            completeness_basis_points: 0,
            freshness: crate::domain::SoftwareProjectionFreshness::Stale,
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
            last_error: None,
        },
    )
}

fn materialize_dependencies(
    connection: &Transaction<'_>,
    source_scope: &str,
) -> Result<(), StorageError> {
    let mut status = require_staged_status(connection, source_scope)?;
    let components =
        dependency_components(connection, source_scope, status.projected_graph_version)?;
    insert_components(connection, &components)?;
    let usages = super::super::dependency_usage::derive_dependency_usages(
        connection,
        source_scope,
        status.projected_graph_version,
        &components,
    )?;
    super::super::dependency_usage::insert_usages(connection, &usages)?;
    status.component_count = components.len();
    upsert_status(connection, &status)
}

fn materialize_sdk_usages(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    let mut status = require_staged_status(connection, source_scope)?;
    let usages = unresolved_sdk_usages(connection, source_scope, status.projected_graph_version)?;
    insert_sdk_usages(connection, &usages)?;
    status.sdk_usage_count = usages.len();
    upsert_status(connection, &status)
}

fn materialize_lifecycle(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    let mut status = require_staged_status(connection, source_scope)?;
    let projection = super::super::lifecycle::refresh_projection(
        connection,
        source_scope,
        status.projected_graph_version,
    )?;
    status.build_target_count = projection.build_targets.len();
    status.iac_resource_count = projection.iac_resources.len();
    status.design_element_count = projection.design_elements.len();
    upsert_status(connection, &status)
}

fn materialize_files(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    let mut status = require_staged_status(connection, source_scope)?;
    status.file_count = super::super::graph::materialize_files(
        connection,
        source_scope,
        status.projected_graph_version,
    )?;
    upsert_status(connection, &status)
}

fn materialize_topics(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    let mut status = require_staged_status(connection, source_scope)?;
    status.topic_count = super::super::graph::materialize_topics(
        connection,
        source_scope,
        status.projected_graph_version,
    )?;
    upsert_status(connection, &status)
}

fn materialize_relationships(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let mut status = require_staged_status(connection, source_scope)?;
    status.relationship_count = super::super::graph::materialize_relationships(
        connection,
        source_scope,
        status.projected_graph_version,
    )?;
    upsert_status(connection, &status)
}

fn materialize_ontology(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    let mut status = require_staged_status(connection, source_scope)?;
    let projection = super::super::ontology::refresh_projection(
        connection,
        source_scope,
        status.projected_graph_version,
    )?;
    status.ontology_version = crate::domain::SOFTWARE_ONTOLOGY_VERSION.to_owned();
    status.projection_schema_version = super::SOFTWARE_PROJECTION_SCHEMA_VERSION as u32;
    status.source_coverage = projection.source_coverage;
    status.completeness_basis_points = projection.completeness_basis_points;
    status.freshness = crate::domain::SoftwareProjectionFreshness::Fresh;
    status.conflict_count = projection.conflict_count;
    status.entity_count = projection.entities.len();
    status.statement_count = projection.statements.len();
    status.diagnostic_count = projection.diagnostics.len();
    upsert_status(connection, &status)
}

fn require_staged_status(
    connection: &Connection,
    source_scope: &str,
) -> Result<SoftwareGlobalStatus, StorageError> {
    let status = status_for_scope(connection, source_scope)?.ok_or_else(|| {
        StorageError::Invariant(format!(
            "software projection for scope '{source_scope}' has not completed its reset phase"
        ))
    })?;
    if !status.stale {
        return Err(StorageError::Invariant(format!(
            "software projection for scope '{source_scope}' became visible before publication"
        )));
    }
    Ok(status)
}

fn validate_fence(
    connection: &Transaction<'_>,
    source_scope: &str,
    fence: &code::lifecycle::publication_fence::PublicationFenceGuard,
) -> Result<(), StorageError> {
    fence.validate_scope_repository(connection, source_scope)?;
    fence.validate_target_scope(connection, source_scope)?;
    fence.validate(connection)
}

fn unfiltered_request(status: &SoftwareGlobalStatus) -> SoftwareGlobalRequest {
    SoftwareGlobalRequest {
        repository: CodeRepositorySelector {
            repository: status.repository_id.clone(),
            ref_selector: status.source_scope.clone(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        },
        kind: SoftwareGlobalKind::All,
        freshness_policy: FreshnessPolicy::AllowStale,
        limit: 1,
    }
}

fn count_dependency_usages(
    connection: &Connection,
    source_scope: &str,
) -> Result<usize, StorageError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM software_dependency_usages WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get(0),
        )
        .map_err(StorageError::from)
}
