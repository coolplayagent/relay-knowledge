//! Transactional business projection persistence.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use crate::{
    domain::{
        BusinessKnowledgeProjectionInput, BusinessKnowledgeSource, BusinessKnowledgeStatus,
        BusinessTechnicalMappingDefinition, BusinessTermDefinition, GraphVersion,
        OntologyEntityKind, OntologyIdentity, SourceScope,
    },
    storage::StorageError,
};

use super::{
    super::{code::lifecycle::publication_fence::PublicationFenceGuard, graph},
    AUTHORED_CONFIDENCE, PROJECTION_SCHEMA_VERSION,
    resolution::resolve_mapping,
};

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
        domain_count: counts.domains,
        term_count: counts.terms,
        mapping_count: counts.mappings,
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
) -> Result<ProjectionCounts, StorageError> {
    let ontology_scope = SourceScope::parse(input.repository_id.clone())
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?;
    let mut counts = ProjectionCounts::default();
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
                "INSERT INTO business_domains (
                    source_scope, repository_id, source_id, source_path, source_digest,
                    authority_rank, domain_id, entity_id, name, description, evidence_id,
                    confidence_basis_points, lifecycle, valid_from_graph_version,
                    valid_until_graph_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'accepted', ?13, NULL)",
                params![
                    input.source_scope,
                    input.repository_id,
                    source.source_id,
                    source.source_path,
                    source.content_digest,
                    source.authority_rank,
                    domain.id,
                    identity.stable_entity_id(),
                    domain.name,
                    domain.description,
                    evidence_id,
                    AUTHORED_CONFIDENCE,
                    graph_version.get()
                ],
            )?;
            counts.domains += 1;
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
                "INSERT INTO business_terms (
                    source_scope, repository_id, source_id, source_path, source_digest,
                    authority_rank, domain_id, term_id, entity_id, canonical_name,
                    definition, language, term_status, semantics_json, evidence_id,
                    confidence_basis_points, lifecycle, valid_from_graph_version,
                    valid_until_graph_version
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'accepted', ?17, NULL)",
                params![
                    input.source_scope,
                    input.repository_id,
                    source.source_id,
                    source.source_path,
                    source.content_digest,
                    source.authority_rank,
                    term.domain,
                    term.id,
                    identity.stable_entity_id(),
                    term.canonical_name,
                    term.definition,
                    term.language,
                    term.status.as_str(),
                    semantics_json,
                    evidence_id,
                    AUTHORED_CONFIDENCE,
                    graph_version.get()
                ],
            )?;
            let context = ProjectionPersistenceContext {
                input,
                source,
                term,
                evidence_id: &evidence_id,
                graph_version,
            };
            persist_aliases(transaction, &context)?;
            for (mapping_index, mapping) in term.mappings.iter().enumerate() {
                persist_mapping(transaction, &context, mapping_index, mapping)?;
                counts.mappings += 1;
            }
            counts.terms += 1;
        }
    }
    Ok(counts)
}

#[derive(Default)]
struct ProjectionCounts {
    domains: usize,
    terms: usize,
    mappings: usize,
}

struct ProjectionPersistenceContext<'a> {
    input: &'a BusinessKnowledgeProjectionInput,
    source: &'a BusinessKnowledgeSource,
    term: &'a BusinessTermDefinition,
    evidence_id: &'a str,
    graph_version: GraphVersion,
}

fn persist_aliases(
    transaction: &Transaction<'_>,
    context: &ProjectionPersistenceContext<'_>,
) -> Result<(), StorageError> {
    for alias in &context.term.aliases {
        transaction.execute(
            "INSERT INTO business_term_aliases (
                source_scope, source_id, domain_id, term_id, alias, alias_kind, language,
                evidence_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                context.input.source_scope,
                context.source.source_id,
                context.term.domain,
                context.term.id,
                alias.value,
                alias.kind.as_str(),
                alias.language,
                context.evidence_id
            ],
        )?;
    }
    Ok(())
}

fn persist_mapping(
    transaction: &Transaction<'_>,
    context: &ProjectionPersistenceContext<'_>,
    mapping_index: usize,
    mapping: &BusinessTechnicalMappingDefinition,
) -> Result<(), StorageError> {
    let resolved_id = resolve_mapping(transaction, &context.input.source_scope, mapping)?;
    let resolution_state = if resolved_id.is_some() {
        "resolved"
    } else {
        "unresolved"
    };
    transaction.execute(
        "INSERT INTO business_mappings (
            source_scope, source_id, domain_id, term_id, mapping_index, relation_kind,
            target_kind, target, target_path, target_source_scope, resolution_state,
            resolved_id, target_hint, evidence_id, confidence_basis_points, lifecycle,
            valid_from_graph_version, valid_until_graph_version
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 'accepted', ?16, NULL)",
        params![
            context.input.source_scope,
            context.source.source_id,
            context.term.domain,
            context.term.id,
            mapping_index,
            mapping.relation.as_str(),
            mapping.target_kind.as_str(),
            mapping.target,
            mapping.path,
            mapping.source_scope,
            resolution_state,
            resolved_id,
            mapping.target,
            context.evidence_id,
            AUTHORED_CONFIDENCE,
            context.graph_version.get()
        ],
    )?;
    Ok(())
}

fn upsert_status(
    connection: &Connection,
    status: &BusinessKnowledgeStatus,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT OR REPLACE INTO business_knowledge_status (
            source_scope, repository_id, resolved_commit_sha, projected_graph_version,
            stale, source_count, domain_count, term_count, mapping_count,
            projection_schema_version, last_error
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)",
        params![
            status.source_scope,
            status.repository_id,
            status.resolved_commit_sha,
            status.projected_graph_version.get(),
            status.stale,
            status.source_count,
            status.domain_count,
            status.term_count,
            status.mapping_count,
            PROJECTION_SCHEMA_VERSION
        ],
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

fn evidence_id(scope: &str, source: &str, entity: &str, discriminator: &str) -> String {
    let mut digest = Sha256::new();
    for value in [scope, source, entity, discriminator] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!("business-evidence:{:x}", digest.finalize())
}
