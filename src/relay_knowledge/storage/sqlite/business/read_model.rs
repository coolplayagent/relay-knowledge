//! Business projection reads, materialization, and query filtering.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    domain::{
        BusinessAlias, BusinessDefinitionFact, BusinessDomain, BusinessKnowledgeConflict,
        BusinessKnowledgeProjection, BusinessKnowledgeQueryKind, BusinessKnowledgeQueryRequest,
        BusinessKnowledgeResolution, BusinessKnowledgeStatus, BusinessTerm, GraphVersion,
        OntologyEntityKind,
    },
    storage::StorageError,
};

use super::row_mapping::{
    EvidenceColumns, TermRow, evidence_from_row, mapping_from_row, ontology_identity,
    parse_alias_kind, parse_term_status,
};

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
            |row| row.get::<_, String>("repository_id"),
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
            "SELECT repository_id, resolved_commit_sha, projected_graph_version, stale,
                    source_count, domain_count, term_count, mapping_count, last_error
             FROM business_knowledge_status WHERE source_scope = ?1",
            params![source_scope],
            |row| {
                Ok(BusinessKnowledgeStatus {
                    repository_id: row.get("repository_id")?,
                    source_scope: source_scope.to_owned(),
                    resolved_commit_sha: row.get("resolved_commit_sha")?,
                    projected_graph_version: GraphVersion::new(
                        row.get("projected_graph_version")?,
                    ),
                    stale: row.get("stale")?,
                    source_count: row.get("source_count")?,
                    domain_count: row.get("domain_count")?,
                    term_count: row.get("term_count")?,
                    mapping_count: row.get("mapping_count")?,
                    last_error: row.get("last_error")?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::InvalidInput(format!(
                "business knowledge projection for repository '{repository}' scope '{source_scope}' is missing"
            ))
        })
}

fn read_domains(
    connection: &Connection,
    source_scope: &str,
    status: &BusinessKnowledgeStatus,
) -> Result<Vec<BusinessDomain>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT source_id, source_path, source_digest, authority_rank, domain_id, entity_id,
                name, description, evidence_id, confidence_basis_points, lifecycle,
                valid_from_graph_version, valid_until_graph_version
         FROM business_domains WHERE source_scope = ?1 ORDER BY authority_rank, domain_id",
    )?;
    let rows = statement.query_map(params![source_scope], |row| {
        let domain_id = row.get::<_, String>("domain_id")?;
        Ok((
            row.get::<_, usize>("authority_rank")?,
            BusinessDomain {
                identity: ontology_identity(
                    &status.repository_id,
                    &domain_id,
                    &domain_id,
                    OntologyEntityKind::BusinessDomain,
                ),
                entity_id: row.get("entity_id")?,
                id: domain_id,
                name: row.get("name")?,
                description: row.get("description")?,
                evidence: evidence_from_row(
                    row,
                    &status.resolved_commit_sha,
                    &EvidenceColumns::BUSINESS_DOMAIN,
                )?,
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
        "SELECT source_id, source_path, source_digest, authority_rank, domain_id, term_id,
                entity_id, canonical_name, definition, language, term_status, semantics_json,
                evidence_id, confidence_basis_points, lifecycle, valid_from_graph_version,
                valid_until_graph_version
         FROM business_terms WHERE source_scope = ?1 ORDER BY authority_rank, domain_id, term_id",
    )?;
    let rows = statement.query_map(params![source_scope], TermRow::from_row)?;
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
            evidence: row.evidence(&projection_status.resolved_commit_sha)?,
        });
        if let Some(value) = row.semantics()? {
            semantics.push(value);
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
        "SELECT DISTINCT alias, alias_kind, language FROM business_term_aliases
         WHERE source_scope = ?1 AND domain_id = ?2 AND term_id = ?3 ORDER BY alias",
    )?;
    statement
        .query_map(params![source_scope, domain_id, term_id], |row| {
            Ok((
                row.get::<_, String>("alias")?,
                row.get::<_, String>("alias_kind")?,
                row.get::<_, Option<String>>("language")?,
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
) -> Result<Vec<crate::domain::BusinessTechnicalMapping>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT source_id, mapping_index, relation_kind, target_kind, target, target_path,
                target_source_scope, resolution_state, resolved_id, target_hint, evidence_id,
                confidence_basis_points, lifecycle, valid_from_graph_version,
                valid_until_graph_version
         FROM business_mappings
         WHERE source_scope = ?1 AND domain_id = ?2 AND term_id = ?3
         ORDER BY source_id, mapping_index",
    )?;
    let rows = statement.query_map(params![source_scope, domain_id, term_id], |row| {
        mapping_from_row(row, &status.resolved_commit_sha)
    })?;
    let mut mappings = Vec::new();
    for row in rows {
        let (source_id, mut mapping) = row?;
        let source = connection.query_row(
            "SELECT source_path, source_digest FROM business_terms
             WHERE source_scope = ?1 AND source_id = ?2 AND domain_id = ?3 AND term_id = ?4",
            params![source_scope, source_id, domain_id, term_id],
            |row| {
                Ok((
                    row.get::<_, String>("source_path")?,
                    row.get::<_, String>("source_digest")?,
                ))
            },
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
