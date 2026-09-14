//! Technical-target resolution and post-projection rebinding.

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{BusinessMappingRelation, BusinessTechnicalMappingDefinition, TechnicalTargetKind},
    storage::StorageError,
};

pub(super) fn resolve_mapping(
    connection: &Connection,
    source_scope: &str,
    mapping: &BusinessTechnicalMappingDefinition,
) -> Result<Option<String>, StorageError> {
    if mapping
        .source_scope
        .as_deref()
        .is_some_and(|declared| declared != source_scope && declared != "repo")
    {
        return Ok(None);
    }
    let resolved = match mapping.target_kind {
        TechnicalTargetKind::File => lookup_target(
            connection,
            "SELECT file_id FROM code_repository_files WHERE source_scope = ?1 AND path = ?2 LIMIT 1",
            source_scope,
            &mapping.target,
        )?,
        TechnicalTargetKind::Symbol => lookup_target_with_path(
            connection,
            "SELECT canonical_symbol_id FROM code_repository_symbols WHERE source_scope = ?1 AND (canonical_symbol_id = ?2 OR name = ?2 OR qualified_name = ?2) AND (?3 IS NULL OR path = ?3) ORDER BY path LIMIT 1",
            source_scope,
            &mapping.target,
            mapping.path.as_deref(),
        )?,
        TechnicalTargetKind::ConfigKey => lookup_target_with_path(
            connection,
            "SELECT feature_flag_id FROM code_repository_feature_flags WHERE source_scope = ?1 AND source_kind != 'config_symbol' AND edge_kind NOT IN ('declares_string_constant','declares_config_getter') AND (name = ?2 OR source_key = ?2) AND (?3 IS NULL OR path = ?3) ORDER BY path LIMIT 1",
            source_scope,
            &mapping.target,
            mapping.path.as_deref(),
        )?,
        TechnicalTargetKind::Api => lookup_target_with_path(
            connection,
            "SELECT route_id FROM code_repository_routes WHERE source_scope = ?1 AND url = ?2 AND (?3 IS NULL OR path = ?3) ORDER BY path LIMIT 1",
            source_scope,
            &mapping.target,
            mapping.path.as_deref(),
        )?,
        TechnicalTargetKind::SoftwareComponent => lookup_target(
            connection,
            "SELECT component_id FROM software_components WHERE source_scope = ?1 AND (component_id = ?2 OR name = ?2) LIMIT 1",
            source_scope,
            &mapping.target,
        )?,
        TechnicalTargetKind::BuildTarget => lookup_target(
            connection,
            "SELECT target_id FROM software_build_targets WHERE source_scope = ?1 AND (target_id = ?2 OR name = ?2) LIMIT 1",
            source_scope,
            &mapping.target,
        )?,
        TechnicalTargetKind::Iac => lookup_target(
            connection,
            "SELECT resource_id FROM software_iac_resources WHERE source_scope = ?1 AND (resource_id = ?2 OR name = ?2) LIMIT 1",
            source_scope,
            &mapping.target,
        )?,
        TechnicalTargetKind::DesignElement => lookup_target(
            connection,
            "SELECT element_id FROM software_design_elements WHERE source_scope = ?1 AND (element_id = ?2 OR name = ?2) LIMIT 1",
            source_scope,
            &mapping.target,
        )?,
        TechnicalTargetKind::DatabaseTable
        | TechnicalTargetKind::DatabaseColumn
        | TechnicalTargetKind::Metric
        | TechnicalTargetKind::External => None,
    };
    Ok(resolved)
}

/// Rebinds authored mappings after software targets are rebuilt transactionally.
pub(in crate::storage::sqlite) fn refresh_mapping_resolutions(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let mappings = {
        let mut statement = connection.prepare(
            "SELECT source_id, domain_id, term_id, mapping_index, relation_kind, target_kind, target, target_path, target_source_scope
             FROM business_mappings
             WHERE source_scope = ?1",
        )?;
        statement
            .query_map(params![source_scope], |row| {
                Ok(StoredMappingIdentity {
                    source_id: row.get("source_id")?,
                    domain_id: row.get("domain_id")?,
                    term_id: row.get("term_id")?,
                    mapping_index: row.get("mapping_index")?,
                    definition: BusinessTechnicalMappingDefinition {
                        relation: parse_relation(&row.get::<_, String>("relation_kind")?)
                            .map_err(super::row_mapping::sql_conversion)?,
                        target_kind: parse_target_kind(&row.get::<_, String>("target_kind")?)
                            .map_err(super::row_mapping::sql_conversion)?,
                        target: row.get("target")?,
                        path: row.get("target_path")?,
                        source_scope: row.get("target_source_scope")?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for mapping in mappings {
        let resolved_id = resolve_mapping(connection, source_scope, &mapping.definition)?;
        connection.execute(
            "UPDATE business_mappings
             SET resolution_state = ?6, resolved_id = ?7
             WHERE source_scope = ?1 AND source_id = ?2 AND domain_id = ?3
               AND term_id = ?4 AND mapping_index = ?5",
            params![
                source_scope,
                mapping.source_id,
                mapping.domain_id,
                mapping.term_id,
                mapping.mapping_index,
                if resolved_id.is_some() {
                    "resolved"
                } else {
                    "unresolved"
                },
                resolved_id,
            ],
        )?;
    }
    Ok(())
}

struct StoredMappingIdentity {
    source_id: String,
    domain_id: String,
    term_id: String,
    mapping_index: usize,
    definition: BusinessTechnicalMappingDefinition,
}

fn lookup_target(
    connection: &Connection,
    sql: &str,
    source_scope: &str,
    target: &str,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(sql, params![source_scope, target], |row| row.get(0))
        .optional()
        .map_err(StorageError::from)
}

fn lookup_target_with_path(
    connection: &Connection,
    sql: &str,
    source_scope: &str,
    target: &str,
    path: Option<&str>,
) -> Result<Option<String>, StorageError> {
    connection
        .query_row(sql, params![source_scope, target, path], |row| row.get(0))
        .optional()
        .map_err(StorageError::from)
}

pub(super) fn parse_relation(value: &str) -> Result<BusinessMappingRelation, StorageError> {
    match value {
        "represented_by" => Ok(BusinessMappingRelation::RepresentedBy),
        "calculated_from" => Ok(BusinessMappingRelation::CalculatedFrom),
        _ => Err(StorageError::Invariant(format!(
            "unknown business relation '{value}'"
        ))),
    }
}

pub(super) fn parse_target_kind(value: &str) -> Result<TechnicalTargetKind, StorageError> {
    match value {
        "file" => Ok(TechnicalTargetKind::File),
        "symbol" => Ok(TechnicalTargetKind::Symbol),
        "config_key" => Ok(TechnicalTargetKind::ConfigKey),
        "api" => Ok(TechnicalTargetKind::Api),
        "software_component" => Ok(TechnicalTargetKind::SoftwareComponent),
        "build_target" => Ok(TechnicalTargetKind::BuildTarget),
        "iac" => Ok(TechnicalTargetKind::Iac),
        "design_element" => Ok(TechnicalTargetKind::DesignElement),
        "database_table" => Ok(TechnicalTargetKind::DatabaseTable),
        "database_column" => Ok(TechnicalTargetKind::DatabaseColumn),
        "metric" => Ok(TechnicalTargetKind::Metric),
        "external" => Ok(TechnicalTargetKind::External),
        _ => Err(StorageError::Invariant(format!(
            "unknown technical target kind '{value}'"
        ))),
    }
}

#[test]
fn business_config_mapping_ignores_internal_registry_candidates() {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE code_repository_feature_flags (source_scope TEXT, feature_flag_id TEXT, name TEXT, source_key TEXT, path TEXT, source_kind TEXT, edge_kind TEXT);
        INSERT INTO code_repository_feature_flags VALUES
        ('scope','internal','flag','flag','a.java','config_symbol','reads_config'),
        ('scope','constant','constant','constant','a.java','config_key','declares_string_constant'),
        ('scope','real','flag','flag','z.properties','config_key','defines_config');").unwrap();
    for (target, expected) in [("flag", Some("real")), ("constant", None)] {
        let mapping = BusinessTechnicalMappingDefinition {
            relation: BusinessMappingRelation::RepresentedBy,
            target_kind: TechnicalTargetKind::ConfigKey,
            target: target.into(),
            path: None,
            source_scope: None,
        };
        assert_eq!(
            resolve_mapping(&connection, "scope", &mapping)
                .unwrap()
                .as_deref(),
            expected
        );
    }
}
