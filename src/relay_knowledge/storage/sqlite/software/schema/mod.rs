use rusqlite::{Connection, params};

use crate::storage::StorageError;

use super::super::schema::columns;
use super::{dependency_usage, lifecycle, ontology};

pub(super) const SOFTWARE_PROJECTION_SCHEMA_VERSION: i64 =
    crate::domain::SOFTWARE_PROJECTION_SCHEMA_VERSION as i64;

pub(in super::super) fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
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
            build_target_count INTEGER NOT NULL DEFAULT 0,
            iac_resource_count INTEGER NOT NULL DEFAULT 0,
            design_element_count INTEGER NOT NULL DEFAULT 0,
            projection_schema_version INTEGER NOT NULL DEFAULT 7,
            ontology_version TEXT NOT NULL DEFAULT '0',
            source_coverage_json TEXT NOT NULL DEFAULT '{\"source_kinds\":[],\"source_path_count\":0,\"evidence_ref_count\":0}',
            completeness_basis_points INTEGER NOT NULL DEFAULT 0,
            freshness TEXT NOT NULL DEFAULT 'stale',
            conflict_count INTEGER NOT NULL DEFAULT 0,
            entity_count INTEGER NOT NULL DEFAULT 0,
            statement_count INTEGER NOT NULL DEFAULT 0,
            diagnostic_count INTEGER NOT NULL DEFAULT 0,
            last_error TEXT
        );
        ",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "file_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "topic_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "relationship_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "projection_schema_version",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "build_target_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "iac_resource_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "design_element_count",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "ontology_version",
        "TEXT NOT NULL DEFAULT '0'",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "source_coverage_json",
        "TEXT NOT NULL DEFAULT '{\"source_kinds\":[],\"source_path_count\":0,\"evidence_ref_count\":0}'",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "completeness_basis_points",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    columns::ensure_column(
        connection,
        "software_global_status",
        "freshness",
        "TEXT NOT NULL DEFAULT 'stale'",
    )?;
    for column in [
        "conflict_count",
        "entity_count",
        "statement_count",
        "diagnostic_count",
    ] {
        columns::ensure_column(
            connection,
            "software_global_status",
            column,
            "INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    mark_legacy_projection_schema_stale(connection)?;
    dependency_usage::initialize_schema(connection)?;
    lifecycle::initialize_schema(connection)?;
    ontology::initialize_schema(connection)
}

fn mark_legacy_projection_schema_stale(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "
        UPDATE software_global_status
        SET stale = 1,
            freshness = 'stale',
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

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
