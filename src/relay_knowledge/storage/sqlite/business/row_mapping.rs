//! Named SQLite row mappings for business read models.

use crate::{
    domain::{
        BusinessEvidence, BusinessSemantics, BusinessTechnicalMapping,
        BusinessTechnicalMappingDefinition, BusinessTermStatus, FactStatus, GraphVersion,
        OntologyEntityKind, OntologyIdentity, SourceScope,
    },
    storage::StorageError,
};

use super::resolution::{parse_relation, parse_target_kind};

pub(super) struct TermRow {
    pub(super) source_id: String,
    pub(super) source_path: String,
    pub(super) source_digest: String,
    pub(super) domain_id: String,
    pub(super) term_id: String,
    pub(super) entity_id: String,
    pub(super) canonical_name: String,
    pub(super) definition: String,
    pub(super) language: String,
    pub(super) status: String,
    pub(super) semantics_json: Option<String>,
    pub(super) evidence_id: String,
    pub(super) confidence: u16,
    pub(super) lifecycle: String,
    pub(super) valid_from: u64,
    pub(super) valid_until: Option<u64>,
}

impl TermRow {
    pub(super) fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            source_id: row.get("source_id")?,
            source_path: row.get("source_path")?,
            source_digest: row.get("source_digest")?,
            domain_id: row.get("domain_id")?,
            term_id: row.get("term_id")?,
            entity_id: row.get("entity_id")?,
            canonical_name: row.get("canonical_name")?,
            definition: row.get("definition")?,
            language: row.get("language")?,
            status: row.get("term_status")?,
            semantics_json: row.get("semantics_json")?,
            evidence_id: row.get("evidence_id")?,
            confidence: row.get("confidence_basis_points")?,
            lifecycle: row.get("lifecycle")?,
            valid_from: row.get("valid_from_graph_version")?,
            valid_until: row.get("valid_until_graph_version")?,
        })
    }

    pub(super) fn evidence(
        &self,
        resolved_commit_sha: &str,
    ) -> Result<BusinessEvidence, StorageError> {
        Ok(BusinessEvidence {
            evidence_id: self.evidence_id.clone(),
            source_id: self.source_id.clone(),
            source_path: self.source_path.clone(),
            source_digest: self.source_digest.clone(),
            resolved_commit_sha: resolved_commit_sha.to_owned(),
            line_start: 1,
            line_end: 1,
            confidence_basis_points: self.confidence,
            lifecycle: FactStatus::parse(&self.lifecycle)
                .map_err(|error| StorageError::Invariant(error.to_string()))?,
            valid_from_graph_version: GraphVersion::new(self.valid_from),
            valid_until_graph_version: self.valid_until.map(GraphVersion::new),
        })
    }

    pub(super) fn semantics(&self) -> Result<Option<BusinessSemantics>, StorageError> {
        self.semantics_json
            .as_deref()
            .map(|json| {
                serde_json::from_str(json).map_err(|error| {
                    StorageError::Invariant(format!("stored business semantics: {error}"))
                })
            })
            .transpose()
    }
}

pub(super) struct EvidenceColumns {
    source_id: &'static str,
    source_path: &'static str,
    source_digest: &'static str,
    evidence_id: &'static str,
    confidence: &'static str,
    lifecycle: &'static str,
    valid_from: &'static str,
    valid_until: &'static str,
}

impl EvidenceColumns {
    pub(super) const BUSINESS_DOMAIN: Self = Self {
        source_id: "source_id",
        source_path: "source_path",
        source_digest: "source_digest",
        evidence_id: "evidence_id",
        confidence: "confidence_basis_points",
        lifecycle: "lifecycle",
        valid_from: "valid_from_graph_version",
        valid_until: "valid_until_graph_version",
    };
}

pub(super) fn evidence_from_row(
    row: &rusqlite::Row<'_>,
    resolved_commit_sha: &str,
    columns: &EvidenceColumns,
) -> rusqlite::Result<BusinessEvidence> {
    Ok(BusinessEvidence {
        evidence_id: row.get(columns.evidence_id)?,
        source_id: row.get(columns.source_id)?,
        source_path: row.get(columns.source_path)?,
        source_digest: row.get(columns.source_digest)?,
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        line_start: 1,
        line_end: 1,
        confidence_basis_points: row.get(columns.confidence)?,
        lifecycle: FactStatus::parse(&row.get::<_, String>(columns.lifecycle)?)
            .map_err(sql_conversion)?,
        valid_from_graph_version: GraphVersion::new(row.get(columns.valid_from)?),
        valid_until_graph_version: row
            .get::<_, Option<u64>>(columns.valid_until)?
            .map(GraphVersion::new),
    })
}

pub(super) fn mapping_from_row(
    row: &rusqlite::Row<'_>,
    resolved_commit_sha: &str,
) -> rusqlite::Result<(String, BusinessTechnicalMapping)> {
    let source_id = row.get::<_, String>("source_id")?;
    Ok((
        source_id.clone(),
        BusinessTechnicalMapping {
            definition: BusinessTechnicalMappingDefinition {
                relation: parse_relation(&row.get::<_, String>("relation_kind")?)
                    .map_err(sql_conversion)?,
                target_kind: parse_target_kind(&row.get::<_, String>("target_kind")?)
                    .map_err(sql_conversion)?,
                target: row.get("target")?,
                path: row.get("target_path")?,
                source_scope: row.get("target_source_scope")?,
            },
            resolution_state: row.get("resolution_state")?,
            resolved_id: row.get("resolved_id")?,
            target_hint: row.get("target_hint")?,
            evidence: BusinessEvidence {
                evidence_id: row.get("evidence_id")?,
                source_id,
                source_path: String::new(),
                source_digest: String::new(),
                resolved_commit_sha: resolved_commit_sha.to_owned(),
                line_start: 1,
                line_end: 1,
                confidence_basis_points: row.get("confidence_basis_points")?,
                lifecycle: FactStatus::parse(&row.get::<_, String>("lifecycle")?)
                    .map_err(sql_conversion)?,
                valid_from_graph_version: GraphVersion::new(row.get("valid_from_graph_version")?),
                valid_until_graph_version: row
                    .get::<_, Option<u64>>("valid_until_graph_version")?
                    .map(GraphVersion::new),
            },
        },
    ))
}

pub(super) fn ontology_identity(
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

pub(super) fn parse_term_status(value: &str) -> Result<BusinessTermStatus, StorageError> {
    match value {
        "active" => Ok(BusinessTermStatus::Active),
        "deprecated" => Ok(BusinessTermStatus::Deprecated),
        _ => Err(StorageError::Invariant(format!(
            "unknown business term status '{value}'"
        ))),
    }
}

pub(super) fn parse_alias_kind(
    value: &str,
) -> Result<crate::domain::BusinessAliasKind, StorageError> {
    match value {
        "synonym" => Ok(crate::domain::BusinessAliasKind::Synonym),
        "abbreviation" => Ok(crate::domain::BusinessAliasKind::Abbreviation),
        _ => Err(StorageError::Invariant(format!(
            "unknown business alias kind '{value}'"
        ))),
    }
}

pub(super) fn sql_conversion(error: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.to_string(),
        )),
    )
}
