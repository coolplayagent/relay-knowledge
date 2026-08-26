//! Repository-scoped business knowledge projection and query owner.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use crate::{
    domain::{
        BusinessAlias, BusinessAliasKind, BusinessDefinitionFact, BusinessDomain, BusinessEvidence,
        BusinessKnowledgeConflict, BusinessKnowledgeProjection, BusinessKnowledgeProjectionInput,
        BusinessKnowledgeQueryKind, BusinessKnowledgeQueryRequest, BusinessKnowledgeResolution,
        BusinessKnowledgeStatus, BusinessMappingRelation, BusinessSemantics,
        BusinessTechnicalMapping, BusinessTechnicalMappingDefinition, BusinessTerm,
        BusinessTermStatus, FactStatus, GraphVersion, OntologyEntityKind, OntologyIdentity,
        SourceScope, TechnicalTargetKind,
    },
    storage::StorageError,
};

use super::{code::lifecycle::publication_fence::PublicationFenceGuard, graph};

mod schema;

pub(in crate::storage::sqlite) use schema::initialize_schema;

const PROJECTION_SCHEMA_VERSION: i64 = 1;
const AUTHORED_CONFIDENCE: u16 = 10_000;

pub(in crate::storage::sqlite) fn replace_projection(
    connection: &mut Connection,
    input: BusinessKnowledgeProjectionInput,
    fence: Option<&PublicationFenceGuard>,
) -> Result<BusinessKnowledgeStatus, StorageError> {
    validate_projection_input(&input)?;
    let graph_version = graph::current_graph_version(connection)?;
    let transaction = connection.transaction()?;
    if let Some(fence) = fence {
        fence.validate_scope_repository(&transaction, &input.source_scope)?;
        fence.validate_target_scope(&transaction, &input.source_scope)?;
        fence.validate(&transaction)?;
    }
    delete_scope(&transaction, &input.source_scope)?;
    let counts = persist_sources(&transaction, &input, graph_version)?;
    let status = BusinessKnowledgeStatus {
        repository_id: input.repository_id.clone(),
        source_scope: input.source_scope.clone(),
        resolved_commit_sha: input.resolved_commit_sha.clone(),
        projected_graph_version: graph_version,
        stale: fence.is_some(),
        source_count: input.sources.len(),
        domain_count: counts.0,
        term_count: counts.1,
        mapping_count: counts.2,
        last_error: None,
    };
    upsert_status(&transaction, &status)?;
    if let Some(fence) = fence {
        fence.validate_target_scope(&transaction, &input.source_scope)?;
        fence.validate(&transaction)?;
    }
    transaction.commit()?;
    Ok(status)
}

fn validate_projection_input(input: &BusinessKnowledgeProjectionInput) -> Result<(), StorageError> {
    if input.repository_id.trim().is_empty()
        || input.source_scope.trim().is_empty()
        || input.resolved_commit_sha.trim().is_empty()
    {
        return Err(StorageError::InvalidInput(
            "business projection repository, scope, and commit must be non-empty".to_owned(),
        ));
    }
    for source in &input.sources {
        source.glossary.validate().map_err(|error| {
            StorageError::InvalidInput(format!("business source '{}': {error}", source.source_id))
        })?;
    }
    Ok(())
}

fn delete_scope(connection: &Connection, source_scope: &str) -> Result<(), StorageError> {
    for table in [
        "business_mappings",
        "business_term_aliases",
        "business_terms",
        "business_domains",
        "business_knowledge_status",
    ] {
        connection.execute(
            &format!("DELETE FROM {table} WHERE source_scope = ?1"),
            params![source_scope],
        )?;
    }
    Ok(())
}

fn persist_sources(
    transaction: &Transaction<'_>,
    input: &BusinessKnowledgeProjectionInput,
    graph_version: GraphVersion,
) -> Result<(usize, usize, usize), StorageError> {
    let ontology_scope = SourceScope::parse(input.repository_id.clone())
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let mut domain_count = 0usize;
    let mut term_count = 0usize;
    let mut mapping_count = 0usize;
    for source in &input.sources {
        for domain in &source.glossary.domains {
            let identity = OntologyIdentity::new(
                ontology_scope.clone(),
                domain.id.clone(),
                domain.id.clone(),
                OntologyEntityKind::BusinessDomain,
            )
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            let evidence_id =
                evidence_id(&input.source_scope, &source.source_id, &domain.id, "domain");
            transaction.execute(
                "INSERT INTO business_domains VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'accepted', ?13, NULL)",
                params![input.source_scope, input.repository_id, source.source_id, source.source_path,
                    source.content_digest, source.authority_rank, domain.id, identity.stable_entity_id(),
                    domain.name, domain.description, evidence_id, AUTHORED_CONFIDENCE,
                    graph_version.get()],
            )?;
            domain_count += 1;
        }
        for term in &source.glossary.terms {
            let identity = OntologyIdentity::new(
                ontology_scope.clone(),
                term.domain.clone(),
                term.id.clone(),
                OntologyEntityKind::BusinessTerm,
            )
            .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
            let evidence_id = evidence_id(
                &input.source_scope,
                &source.source_id,
                &term.id,
                &term.domain,
            );
            let semantics_json = term
                .semantics
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|error| StorageError::Invariant(error.to_string()))?;
            transaction.execute(
                "INSERT INTO business_terms VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'accepted', ?17, NULL)",
                params![input.source_scope, input.repository_id, source.source_id, source.source_path,
                    source.content_digest, source.authority_rank, term.domain, term.id,
                    identity.stable_entity_id(), term.canonical_name, term.definition,
                    term.language, term.status.as_str(), semantics_json, evidence_id,
                    AUTHORED_CONFIDENCE, graph_version.get()],
            )?;
            persist_aliases(transaction, input, source, term, &evidence_id)?;
            for (mapping_index, mapping) in term.mappings.iter().enumerate() {
                persist_mapping(
                    transaction,
                    input,
                    source,
                    term,
                    mapping_index,
                    mapping,
                    &evidence_id,
                    graph_version,
                )?;
                mapping_count += 1;
            }
            term_count += 1;
        }
    }
    Ok((domain_count, term_count, mapping_count))
}

fn persist_aliases(
    transaction: &Transaction<'_>,
    input: &BusinessKnowledgeProjectionInput,
    source: &crate::domain::BusinessKnowledgeSource,
    term: &crate::domain::BusinessTermDefinition,
    evidence_id: &str,
) -> Result<(), StorageError> {
    for alias in &term.aliases {
        transaction.execute(
            "INSERT INTO business_term_aliases VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                input.source_scope,
                source.source_id,
                term.domain,
                term.id,
                alias.value,
                alias.kind.as_str(),
                alias.language,
                evidence_id
            ],
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn persist_mapping(
    transaction: &Transaction<'_>,
    input: &BusinessKnowledgeProjectionInput,
    source: &crate::domain::BusinessKnowledgeSource,
    term: &crate::domain::BusinessTermDefinition,
    mapping_index: usize,
    mapping: &BusinessTechnicalMappingDefinition,
    evidence_id: &str,
    graph_version: GraphVersion,
) -> Result<(), StorageError> {
    let resolved_id = resolve_mapping(transaction, &input.source_scope, mapping)?;
    let resolution_state = if resolved_id.is_some() {
        "resolved"
    } else {
        "unresolved"
    };
    transaction.execute(
        "INSERT INTO business_mappings VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'accepted', ?16, NULL)",
        params![input.source_scope, source.source_id, term.domain, term.id, mapping_index,
            mapping.relation.as_str(), mapping.target_kind.as_str(), mapping.target, mapping.path,
            mapping.source_scope, resolution_state, resolved_id, mapping.target, evidence_id,
            AUTHORED_CONFIDENCE, graph_version.get()],
    )?;
    Ok(())
}

fn resolve_mapping(
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
            "SELECT feature_flag_id FROM code_repository_feature_flags WHERE source_scope = ?1 AND (name = ?2 OR source_key = ?2) AND (?3 IS NULL OR path = ?3) ORDER BY path LIMIT 1",
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

/// Rebinds authored mappings after the software projection has been rebuilt in
/// the same publication transaction. Business facts are staged first so they
/// participate in the publication fence; software-owned targets only become
/// visible later in that transaction.
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
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, usize>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for (source_id, domain_id, term_id, index, relation, kind, target, path, target_scope) in
        mappings
    {
        let definition = BusinessTechnicalMappingDefinition {
            relation: parse_relation(&relation)?,
            target_kind: parse_target_kind(&kind)?,
            target,
            path,
            source_scope: target_scope,
        };
        let resolved_id = resolve_mapping(connection, source_scope, &definition)?;
        connection.execute(
            "UPDATE business_mappings
             SET resolution_state = ?6, resolved_id = ?7
             WHERE source_scope = ?1 AND source_id = ?2 AND domain_id = ?3
               AND term_id = ?4 AND mapping_index = ?5",
            params![
                source_scope,
                source_id,
                domain_id,
                term_id,
                index,
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

fn upsert_status(
    connection: &Connection,
    status: &BusinessKnowledgeStatus,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT OR REPLACE INTO business_knowledge_status VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
        params![status.source_scope, status.repository_id, status.resolved_commit_sha,
            status.projected_graph_version.get(), status.stale, status.source_count,
            status.domain_count, status.term_count, status.mapping_count,
            PROJECTION_SCHEMA_VERSION],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite) fn mark_published(
    connection: &Connection,
    source_scope: &str,
) -> Result<(), StorageError> {
    let staged = connection
        .query_row(
            "SELECT stale = 1 FROM business_knowledge_status WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false);
    if !staged {
        return Err(StorageError::InvalidInput(format!(
            "code scope '{source_scope}' cannot publish before its fenced business projection is complete"
        )));
    }
    connection.execute(
        "UPDATE business_knowledge_status SET stale = 0 WHERE source_scope = ?1",
        params![source_scope],
    )?;
    Ok(())
}

pub(in crate::storage::sqlite) fn projection_for_scope(
    connection: &Connection,
    source_scope: &str,
    request: BusinessKnowledgeQueryRequest,
) -> Result<BusinessKnowledgeProjection, StorageError> {
    let status = read_status(connection, source_scope, &request.repository.repository)?;
    let domains = read_domains(connection, source_scope, &status)?;
    let mut terms = read_terms(connection, source_scope, &status)?;
    let resolution = filter_terms(&mut terms, &domains, &request);
    terms.truncate(request.limit);
    match request.kind {
        BusinessKnowledgeQueryKind::Terms => {
            for term in &mut terms {
                term.mappings.clear();
            }
        }
        BusinessKnowledgeQueryKind::Mappings => {
            for term in &mut terms {
                term.definitions.clear();
                term.semantics.clear();
                term.conflicts.clear();
            }
            terms.retain(|term| !term.mappings.is_empty());
        }
        BusinessKnowledgeQueryKind::All => {}
    }
    let selected_domains = domains
        .into_iter()
        .filter(|domain| {
            request.domain.is_none()
                || terms.iter().any(|term| term.domain_id == domain.id)
                || request.domain.as_ref().is_some_and(|value| {
                    domain.id.eq_ignore_ascii_case(value) || domain.name.eq_ignore_ascii_case(value)
                })
        })
        .collect();
    Ok(BusinessKnowledgeProjection {
        status,
        resolution,
        domains: selected_domains,
        terms,
    })
}

pub(in crate::storage::sqlite) fn status_for_scope(
    connection: &Connection,
    source_scope: &str,
) -> Result<Option<BusinessKnowledgeStatus>, StorageError> {
    let repository = connection
        .query_row(
            "SELECT repository_id FROM business_knowledge_status WHERE source_scope = ?1",
            params![source_scope],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    repository
        .map(|repository| read_status(connection, source_scope, &repository))
        .transpose()
}

fn read_status(
    connection: &Connection,
    source_scope: &str,
    repository: &str,
) -> Result<BusinessKnowledgeStatus, StorageError> {
    connection
        .query_row(
            "SELECT repository_id, resolved_commit_sha, projected_graph_version, stale, source_count, domain_count, term_count, mapping_count, last_error FROM business_knowledge_status WHERE source_scope = ?1",
            params![source_scope],
            |row| Ok(BusinessKnowledgeStatus {
                repository_id: row.get(0)?, source_scope: source_scope.to_owned(),
                resolved_commit_sha: row.get(1)?, projected_graph_version: GraphVersion::new(row.get(2)?),
                stale: row.get(3)?, source_count: row.get(4)?, domain_count: row.get(5)?,
                term_count: row.get(6)?, mapping_count: row.get(7)?, last_error: row.get(8)?,
            }),
        )
        .optional()?
        .ok_or_else(|| StorageError::InvalidInput(format!(
            "business knowledge projection for repository '{repository}' scope '{source_scope}' is missing"
        )))
}

fn read_domains(
    connection: &Connection,
    source_scope: &str,
    status: &BusinessKnowledgeStatus,
) -> Result<Vec<BusinessDomain>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT source_id, source_path, source_digest, authority_rank, domain_id, entity_id, name, description, evidence_id, confidence_basis_points, lifecycle, valid_from_graph_version, valid_until_graph_version FROM business_domains WHERE source_scope = ?1 ORDER BY authority_rank, domain_id",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        let domain_id = row.get::<_, String>(4)?;
        Ok((
            row.get::<_, usize>(3)?,
            BusinessDomain {
                identity: ontology_identity(
                    &status.repository_id,
                    &domain_id,
                    &domain_id,
                    OntologyEntityKind::BusinessDomain,
                ),
                entity_id: row.get(5)?,
                id: domain_id,
                name: row.get(6)?,
                description: row.get(7)?,
                evidence: evidence_from_row(row, status, 0, 1, 2, 8, 9, 10, 11, 12)?,
            },
        ))
    })?;
    let mut preferred = BTreeMap::new();
    for row in rows {
        let (rank, domain) = row?;
        preferred.entry(domain.id.clone()).or_insert((rank, domain));
    }
    Ok(preferred.into_values().map(|(_, domain)| domain).collect())
}

fn read_terms(
    connection: &Connection,
    source_scope: &str,
    status: &BusinessKnowledgeStatus,
) -> Result<Vec<BusinessTerm>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT source_id, source_path, source_digest, authority_rank, domain_id, term_id, entity_id, canonical_name, definition, language, term_status, semantics_json, evidence_id, confidence_basis_points, lifecycle, valid_from_graph_version, valid_until_graph_version FROM business_terms WHERE source_scope = ?1 ORDER BY authority_rank, domain_id, term_id",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        Ok(TermRow {
            source_id: row.get(0)?,
            source_path: row.get(1)?,
            source_digest: row.get(2)?,
            domain_id: row.get(4)?,
            term_id: row.get(5)?,
            entity_id: row.get(6)?,
            canonical_name: row.get(7)?,
            definition: row.get(8)?,
            language: row.get(9)?,
            status: row.get(10)?,
            semantics_json: row.get(11)?,
            evidence_id: row.get(12)?,
            confidence: row.get(13)?,
            lifecycle: row.get(14)?,
            valid_from: row.get(15)?,
            valid_until: row.get(16)?,
        })
    })?;
    let mut grouped: BTreeMap<(String, String), Vec<TermRow>> = BTreeMap::new();
    for row in rows {
        let row = row?;
        grouped
            .entry((row.domain_id.clone(), row.term_id.clone()))
            .or_default()
            .push(row);
    }
    grouped
        .into_values()
        .map(|rows| materialize_term(connection, source_scope, status, rows))
        .collect()
}

struct TermRow {
    source_id: String,
    source_path: String,
    source_digest: String,
    domain_id: String,
    term_id: String,
    entity_id: String,
    canonical_name: String,
    definition: String,
    language: String,
    status: String,
    semantics_json: Option<String>,
    evidence_id: String,
    confidence: u16,
    lifecycle: String,
    valid_from: u64,
    valid_until: Option<u64>,
}

fn materialize_term(
    connection: &Connection,
    source_scope: &str,
    projection_status: &BusinessKnowledgeStatus,
    rows: Vec<TermRow>,
) -> Result<BusinessTerm, StorageError> {
    let preferred = &rows[0];
    let mut definitions = Vec::with_capacity(rows.len());
    let mut values = BTreeSet::new();
    let mut semantics = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        values.insert(row.definition.clone());
        definitions.push(BusinessDefinitionFact {
            definition: row.definition.clone(),
            preferred: index == 0,
            evidence: evidence_from_term(row, projection_status)?,
        });
        if let Some(json) = &row.semantics_json {
            semantics.push(
                serde_json::from_str::<BusinessSemantics>(json).map_err(|error| {
                    StorageError::Invariant(format!("stored business semantics: {error}"))
                })?,
            );
        }
    }
    let conflicts = if values.len() > 1 {
        vec![BusinessKnowledgeConflict {
            predicate: "definition".to_owned(),
            competing_values: values.into_iter().collect(),
            evidence_ids: definitions
                .iter()
                .map(|fact| fact.evidence.evidence_id.clone())
                .collect(),
        }]
    } else {
        Vec::new()
    };
    Ok(BusinessTerm {
        identity: ontology_identity(
            &projection_status.repository_id,
            &preferred.domain_id,
            &preferred.term_id,
            OntologyEntityKind::BusinessTerm,
        ),
        entity_id: preferred.entity_id.clone(),
        id: preferred.term_id.clone(),
        domain_id: preferred.domain_id.clone(),
        canonical_name: preferred.canonical_name.clone(),
        language: preferred.language.clone(),
        status: parse_term_status(&preferred.status)?,
        definitions,
        aliases: read_aliases(
            connection,
            source_scope,
            &preferred.domain_id,
            &preferred.term_id,
        )?,
        semantics,
        conflicts,
        mappings: read_mappings(
            connection,
            source_scope,
            projection_status,
            &preferred.domain_id,
            &preferred.term_id,
        )?,
    })
}

fn read_aliases(
    connection: &Connection,
    source_scope: &str,
    domain_id: &str,
    term_id: &str,
) -> Result<Vec<BusinessAlias>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT alias, alias_kind, language FROM business_term_aliases WHERE source_scope = ?1 AND domain_id = ?2 AND term_id = ?3 ORDER BY alias",
    )?;
    statement
        .query_map(params![source_scope, domain_id, term_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(value, kind, language)| {
            Ok(BusinessAlias {
                value,
                kind: parse_alias_kind(&kind)?,
                language,
            })
        })
        .collect()
}

fn read_mappings(
    connection: &Connection,
    source_scope: &str,
    status: &BusinessKnowledgeStatus,
    domain_id: &str,
    term_id: &str,
) -> Result<Vec<BusinessTechnicalMapping>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT source_id, mapping_index, relation_kind, target_kind, target, target_path, target_source_scope, resolution_state, resolved_id, target_hint, evidence_id, confidence_basis_points, lifecycle, valid_from_graph_version, valid_until_graph_version FROM business_mappings WHERE source_scope = ?1 AND domain_id = ?2 AND term_id = ?3 ORDER BY source_id, mapping_index",
    )?;
    let rows = statement.query_map(params![source_scope, domain_id, term_id], |row| {
        let source_id = row.get::<_, String>(0)?;
        let evidence_id = row.get::<_, String>(10)?;
        Ok((
            source_id,
            BusinessTechnicalMapping {
                definition: BusinessTechnicalMappingDefinition {
                    relation: parse_relation(&row.get::<_, String>(2)?).map_err(sql_conversion)?,
                    target_kind: parse_target_kind(&row.get::<_, String>(3)?)
                        .map_err(sql_conversion)?,
                    target: row.get(4)?,
                    path: row.get(5)?,
                    source_scope: row.get(6)?,
                },
                resolution_state: row.get(7)?,
                resolved_id: row.get(8)?,
                target_hint: row.get(9)?,
                evidence: BusinessEvidence {
                    evidence_id,
                    source_id: String::new(),
                    source_path: String::new(),
                    source_digest: String::new(),
                    resolved_commit_sha: status.resolved_commit_sha.clone(),
                    line_start: 1,
                    line_end: 1,
                    confidence_basis_points: row.get(11)?,
                    lifecycle: FactStatus::parse(&row.get::<_, String>(12)?)
                        .map_err(sql_conversion)?,
                    valid_from_graph_version: GraphVersion::new(row.get(13)?),
                    valid_until_graph_version: row
                        .get::<_, Option<u64>>(14)?
                        .map(GraphVersion::new),
                },
            },
        ))
    })?;
    let mut mappings = Vec::new();
    for row in rows {
        let (source_id, mut mapping) = row?;
        mapping.evidence.source_id = source_id.clone();
        let source = connection.query_row(
            "SELECT source_path, source_digest FROM business_terms WHERE source_scope = ?1 AND source_id = ?2 AND domain_id = ?3 AND term_id = ?4",
            params![source_scope, source_id, domain_id, term_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        mapping.evidence.source_path = source.0;
        mapping.evidence.source_digest = source.1;
        mappings.push(mapping);
    }
    Ok(mappings)
}

fn filter_terms(
    terms: &mut Vec<BusinessTerm>,
    domains: &[BusinessDomain],
    request: &BusinessKnowledgeQueryRequest,
) -> BusinessKnowledgeResolution {
    if let Some(domain) = &request.domain {
        let matching = domains
            .iter()
            .filter(|candidate| {
                candidate.id.eq_ignore_ascii_case(domain)
                    || candidate.name.eq_ignore_ascii_case(domain)
            })
            .map(|candidate| candidate.id.as_str())
            .collect::<BTreeSet<_>>();
        terms.retain(|term| matching.contains(term.domain_id.as_str()));
    }
    let Some(query) = request.query.as_ref() else {
        return BusinessKnowledgeResolution::List;
    };
    let folded = query.to_lowercase();
    let exact = terms
        .iter()
        .filter(|term| {
            term.canonical_name.eq_ignore_ascii_case(query)
                || term
                    .aliases
                    .iter()
                    .any(|alias| alias.value.eq_ignore_ascii_case(query))
        })
        .map(|term| (term.domain_id.clone(), term.id.clone()))
        .collect::<BTreeSet<_>>();
    if !exact.is_empty() {
        terms.retain(|term| exact.contains(&(term.domain_id.clone(), term.id.clone())));
        return if exact.len() > 1 && request.domain.is_none() {
            BusinessKnowledgeResolution::Ambiguous
        } else {
            BusinessKnowledgeResolution::Exact
        };
    }
    terms.retain(|term| {
        term.canonical_name.to_lowercase().contains(&folded)
            || term
                .aliases
                .iter()
                .any(|alias| alias.value.to_lowercase().contains(&folded))
            || term
                .definitions
                .iter()
                .any(|fact| fact.definition.to_lowercase().contains(&folded))
            || term
                .mappings
                .iter()
                .any(|mapping| mapping.target_hint.to_lowercase().contains(&folded))
    });
    if terms.is_empty() {
        BusinessKnowledgeResolution::NotFound
    } else {
        BusinessKnowledgeResolution::List
    }
}

fn evidence_from_term(
    row: &TermRow,
    status: &BusinessKnowledgeStatus,
) -> Result<BusinessEvidence, StorageError> {
    Ok(BusinessEvidence {
        evidence_id: row.evidence_id.clone(),
        source_id: row.source_id.clone(),
        source_path: row.source_path.clone(),
        source_digest: row.source_digest.clone(),
        resolved_commit_sha: status.resolved_commit_sha.clone(),
        line_start: 1,
        line_end: 1,
        confidence_basis_points: row.confidence,
        lifecycle: FactStatus::parse(&row.lifecycle)
            .map_err(|error| StorageError::Invariant(error.to_string()))?,
        valid_from_graph_version: GraphVersion::new(row.valid_from),
        valid_until_graph_version: row.valid_until.map(GraphVersion::new),
    })
}

#[allow(clippy::too_many_arguments)]
fn evidence_from_row(
    row: &rusqlite::Row<'_>,
    status: &BusinessKnowledgeStatus,
    source_id: usize,
    source_path: usize,
    source_digest: usize,
    evidence_id: usize,
    confidence: usize,
    lifecycle: usize,
    valid_from: usize,
    valid_until: usize,
) -> rusqlite::Result<BusinessEvidence> {
    Ok(BusinessEvidence {
        evidence_id: row.get(evidence_id)?,
        source_id: row.get(source_id)?,
        source_path: row.get(source_path)?,
        source_digest: row.get(source_digest)?,
        resolved_commit_sha: status.resolved_commit_sha.clone(),
        line_start: 1,
        line_end: 1,
        confidence_basis_points: row.get(confidence)?,
        lifecycle: FactStatus::parse(&row.get::<_, String>(lifecycle)?).map_err(sql_conversion)?,
        valid_from_graph_version: GraphVersion::new(row.get(valid_from)?),
        valid_until_graph_version: row
            .get::<_, Option<u64>>(valid_until)?
            .map(GraphVersion::new),
    })
}

fn ontology_identity(
    repository_id: &str,
    domain_id: &str,
    entity_id: &str,
    kind: OntologyEntityKind,
) -> OntologyIdentity {
    OntologyIdentity::new(
        SourceScope::parse(repository_id).expect("stored repository id must be valid"),
        domain_id.to_owned(),
        entity_id.to_owned(),
        kind,
    )
    .expect("stored ontology identity must be valid")
}

fn evidence_id(scope: &str, source: &str, entity: &str, discriminator: &str) -> String {
    let mut digest = Sha256::new();
    for value in [scope, source, entity, discriminator] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!("business-evidence:{:x}", digest.finalize())
}

fn parse_term_status(value: &str) -> Result<BusinessTermStatus, StorageError> {
    match value {
        "active" => Ok(BusinessTermStatus::Active),
        "deprecated" => Ok(BusinessTermStatus::Deprecated),
        _ => Err(StorageError::Invariant(format!(
            "unknown business term status '{value}'"
        ))),
    }
}

fn parse_alias_kind(value: &str) -> Result<BusinessAliasKind, StorageError> {
    match value {
        "synonym" => Ok(BusinessAliasKind::Synonym),
        "abbreviation" => Ok(BusinessAliasKind::Abbreviation),
        _ => Err(StorageError::Invariant(format!(
            "unknown business alias kind '{value}'"
        ))),
    }
}

fn parse_relation(value: &str) -> Result<BusinessMappingRelation, StorageError> {
    match value {
        "represented_by" => Ok(BusinessMappingRelation::RepresentedBy),
        "calculated_from" => Ok(BusinessMappingRelation::CalculatedFrom),
        _ => Err(StorageError::Invariant(format!(
            "unknown business relation '{value}'"
        ))),
    }
}

fn parse_target_kind(value: &str) -> Result<TechnicalTargetKind, StorageError> {
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

fn sql_conversion(error: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
