//! SQLite schema ownership for repository-scoped business projections.

use rusqlite::{Connection, params};

use crate::storage::StorageError;

use super::PROJECTION_SCHEMA_VERSION;

pub(in crate::storage::sqlite) fn initialize_schema(
    connection: &Connection,
) -> Result<(), StorageError> {
    connection.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS business_domains (
            source_scope TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            source_path TEXT NOT NULL,
            source_digest TEXT NOT NULL,
            authority_rank INTEGER NOT NULL,
            domain_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            evidence_id TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            lifecycle TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            PRIMARY KEY (source_scope, source_id, domain_id)
        );
        CREATE INDEX IF NOT EXISTS business_domains_lookup
            ON business_domains(source_scope, domain_id, name, authority_rank);

        CREATE TABLE IF NOT EXISTS business_terms (
            source_scope TEXT NOT NULL,
            repository_id TEXT NOT NULL,
            source_id TEXT NOT NULL,
            source_path TEXT NOT NULL,
            source_digest TEXT NOT NULL,
            authority_rank INTEGER NOT NULL,
            domain_id TEXT NOT NULL,
            term_id TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            canonical_name TEXT NOT NULL,
            definition TEXT NOT NULL,
            language TEXT NOT NULL,
            term_status TEXT NOT NULL,
            semantics_json TEXT,
            evidence_id TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            lifecycle TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            PRIMARY KEY (source_scope, source_id, domain_id, term_id)
        );
        CREATE INDEX IF NOT EXISTS business_terms_lookup
            ON business_terms(source_scope, domain_id, canonical_name, authority_rank);

        CREATE TABLE IF NOT EXISTS business_term_aliases (
            source_scope TEXT NOT NULL,
            source_id TEXT NOT NULL,
            domain_id TEXT NOT NULL,
            term_id TEXT NOT NULL,
            alias TEXT NOT NULL,
            alias_kind TEXT NOT NULL,
            language TEXT,
            evidence_id TEXT NOT NULL,
            PRIMARY KEY (source_scope, source_id, domain_id, term_id, alias)
        );
        CREATE INDEX IF NOT EXISTS business_aliases_lookup
            ON business_term_aliases(source_scope, alias, domain_id);

        CREATE TABLE IF NOT EXISTS business_mappings (
            source_scope TEXT NOT NULL,
            source_id TEXT NOT NULL,
            domain_id TEXT NOT NULL,
            term_id TEXT NOT NULL,
            mapping_index INTEGER NOT NULL,
            relation_kind TEXT NOT NULL,
            target_kind TEXT NOT NULL,
            target TEXT NOT NULL,
            target_path TEXT,
            target_source_scope TEXT,
            resolution_state TEXT NOT NULL,
            resolved_id TEXT,
            target_hint TEXT NOT NULL,
            evidence_id TEXT NOT NULL,
            confidence_basis_points INTEGER NOT NULL,
            lifecycle TEXT NOT NULL,
            valid_from_graph_version INTEGER NOT NULL,
            valid_until_graph_version INTEGER,
            PRIMARY KEY (source_scope, source_id, domain_id, term_id, mapping_index)
        );
        CREATE INDEX IF NOT EXISTS business_mappings_lookup
            ON business_mappings(source_scope, target_kind, target, resolution_state);

        CREATE TABLE IF NOT EXISTS business_knowledge_status (
            source_scope TEXT PRIMARY KEY,
            repository_id TEXT NOT NULL,
            resolved_commit_sha TEXT NOT NULL,
            projected_graph_version INTEGER NOT NULL,
            stale INTEGER NOT NULL,
            source_count INTEGER NOT NULL,
            domain_count INTEGER NOT NULL,
            term_count INTEGER NOT NULL,
            mapping_count INTEGER NOT NULL,
            projection_schema_version INTEGER NOT NULL,
            last_error TEXT
        );
        ",
    )?;
    connection.execute(
        "UPDATE business_knowledge_status SET stale = 1, projection_schema_version = ?1 WHERE projection_schema_version != ?1",
        params![PROJECTION_SCHEMA_VERSION],
    )?;
    Ok(())
}
