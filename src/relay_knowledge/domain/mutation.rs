use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{
    DomainError, EvidenceExtractionMetadata, GraphVersion, SourceScope, error::required_text,
};

/// Lifecycle status for graph facts created from evidence or extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
}

impl FactStatus {
    /// Stable storage and API representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
        }
    }

    /// Parses the stable storage and API representation.
    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "proposed" => Ok(Self::Proposed),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "superseded" => Ok(Self::Superseded),
            _ => Err(DomainError::invalid("fact_status", "unknown fact status")),
        }
    }
}

/// Confidence score in basis points so it remains sortable and exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfidenceScore {
    pub basis_points: u16,
}

impl ConfidenceScore {
    pub const CERTAIN: Self = Self {
        basis_points: 10_000,
    };

    /// Validates a confidence score from 0.0 through 1.0.
    pub fn from_ratio(value: f32) -> Result<Self, DomainError> {
        if !(0.0..=1.0).contains(&value) || value.is_nan() {
            return Err(DomainError::invalid(
                "confidence",
                "must be between 0.0 and 1.0",
            ));
        }

        Ok(Self {
            basis_points: (value * 10_000.0).round() as u16,
        })
    }

    /// Revalidates scores that may have been deserialized from public fields.
    pub fn validate(self) -> Result<Self, DomainError> {
        if self.basis_points > 10_000 {
            return Err(DomainError::invalid(
                "confidence",
                "must be between 0 and 10000 basis points",
            ));
        }

        Ok(self)
    }
}

/// Source byte and line span for evidence-backed facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub end_line: u32,
}

impl EvidenceSpan {
    /// Validates a half-open byte span and one-based line coordinates.
    pub fn new(
        start_byte: u32,
        end_byte: u32,
        start_line: u32,
        end_line: u32,
    ) -> Result<Self, DomainError> {
        if end_byte <= start_byte {
            return Err(DomainError::invalid(
                "evidence_span",
                "end byte must be greater than start byte",
            ));
        }
        if start_line == 0 {
            return Err(DomainError::invalid(
                "evidence_span",
                "start line must be one-based",
            ));
        }
        if end_line < start_line {
            return Err(DomainError::invalid(
                "evidence_span",
                "end line must not be before start line",
            ));
        }

        Ok(Self {
            start_byte,
            end_byte,
            start_line,
            end_line,
        })
    }
}

/// Graph-version validity range for facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphVersionRange {
    pub valid_from: GraphVersion,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<GraphVersion>,
}

impl GraphVersionRange {
    /// Creates an open-ended range beginning at a graph version.
    pub const fn open_from(valid_from: GraphVersion) -> Self {
        Self {
            valid_from,
            valid_until: None,
        }
    }

    /// Validates that a bounded range is ordered.
    pub fn new(
        valid_from: GraphVersion,
        valid_until: Option<GraphVersion>,
    ) -> Result<Self, DomainError> {
        if valid_until.is_some_and(|until| until < valid_from) {
            return Err(DomainError::invalid(
                "version_range",
                "valid_until must not be before valid_from",
            ));
        }

        Ok(Self {
            valid_from,
            valid_until,
        })
    }
}

/// Evidence accepted by the graph mutation pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub source_scope: SourceScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<EvidenceSpan>,
    pub content: String,
    pub entity_labels: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub extraction: EvidenceExtractionMetadata,
}

impl EvidenceRecord {
    /// Validates evidence and normalizes entity labels before persistence.
    pub fn new(
        id: impl Into<String>,
        source_scope: SourceScope,
        content: impl Into<String>,
        entity_labels: Vec<String>,
    ) -> Result<Self, DomainError> {
        let normalized_labels = normalize_entity_labels(entity_labels)?;

        Ok(Self {
            id: required_text("evidence_id", id)?,
            source_scope,
            source_path: None,
            span: None,
            content: required_text("evidence_content", content)?,
            entity_labels: normalized_labels,
            confidence: ConfidenceScore::CERTAIN,
            status: FactStatus::Accepted,
            extraction: EvidenceExtractionMetadata::text_span(),
        })
    }

    /// Attaches source-location and confidence metadata to evidence.
    pub fn with_metadata(
        mut self,
        source_path: Option<String>,
        span: Option<EvidenceSpan>,
        confidence: ConfidenceScore,
        status: FactStatus,
    ) -> Result<Self, DomainError> {
        self.source_path = source_path
            .map(|path| required_text("source_path", path))
            .transpose()?;
        self.span = span.map(validate_span).transpose()?;
        self.confidence = confidence.validate()?;
        self.status = status;

        Ok(self)
    }

    /// Attaches validated multimodal extraction metadata to the evidence.
    pub fn with_extraction_metadata(
        mut self,
        extraction: EvidenceExtractionMetadata,
    ) -> Result<Self, DomainError> {
        self.extraction = extraction.validate()?;

        Ok(self)
    }
}

/// Typed relation between graph entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRelationRecord {
    pub id: String,
    pub source_scope: SourceScope,
    pub source_entity_label: String,
    pub relation_type: String,
    pub target_entity_label: String,
    pub evidence_ids: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub version_range: GraphVersionRange,
}

/// Claim fact extracted from or supported by evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub id: String,
    pub source_scope: SourceScope,
    pub subject_entity_label: String,
    pub predicate: String,
    pub object: String,
    pub evidence_ids: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub version_range: GraphVersionRange,
}

/// Event fact tied to entities and optional valid-time text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    pub source_scope: SourceScope,
    pub event_type: String,
    pub entity_labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ConfidenceScore,
    pub status: FactStatus,
    pub version_range: GraphVersionRange,
}

impl GraphRelationRecord {
    /// Validates a typed relation and its supporting evidence references.
    pub fn new(
        id: impl Into<String>,
        source_scope: SourceScope,
        source_entity_label: impl Into<String>,
        relation_type: impl Into<String>,
        target_entity_label: impl Into<String>,
        evidence_ids: Vec<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id: required_text("relation_id", id)?,
            source_scope,
            source_entity_label: required_text("source_entity_label", source_entity_label)?,
            relation_type: required_text("relation_type", relation_type)?,
            target_entity_label: required_text("target_entity_label", target_entity_label)?,
            evidence_ids: normalize_evidence_ids(evidence_ids)?,
            confidence: ConfidenceScore::CERTAIN,
            status: FactStatus::Accepted,
            version_range: GraphVersionRange::open_from(GraphVersion::ZERO),
        })
    }

    /// Applies quality and lifecycle metadata after structural validation.
    pub fn with_metadata(
        mut self,
        confidence: ConfidenceScore,
        status: FactStatus,
        version_range: GraphVersionRange,
    ) -> Result<Self, DomainError> {
        self.confidence = confidence.validate()?;
        self.status = status;
        self.version_range = validate_version_range(version_range)?;

        Ok(self)
    }
}

impl ClaimRecord {
    /// Validates a claim and its supporting evidence references.
    pub fn new(
        id: impl Into<String>,
        source_scope: SourceScope,
        subject_entity_label: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<String>,
        evidence_ids: Vec<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id: required_text("claim_id", id)?,
            source_scope,
            subject_entity_label: required_text("subject_entity_label", subject_entity_label)?,
            predicate: required_text("claim_predicate", predicate)?,
            object: required_text("claim_object", object)?,
            evidence_ids: normalize_evidence_ids(evidence_ids)?,
            confidence: ConfidenceScore::CERTAIN,
            status: FactStatus::Accepted,
            version_range: GraphVersionRange::open_from(GraphVersion::ZERO),
        })
    }

    /// Applies quality and lifecycle metadata after structural validation.
    pub fn with_metadata(
        mut self,
        confidence: ConfidenceScore,
        status: FactStatus,
        version_range: GraphVersionRange,
    ) -> Result<Self, DomainError> {
        self.confidence = confidence.validate()?;
        self.status = status;
        self.version_range = validate_version_range(version_range)?;

        Ok(self)
    }
}

impl EventRecord {
    /// Validates an event fact, linked entities, and supporting evidence.
    pub fn new(
        id: impl Into<String>,
        source_scope: SourceScope,
        event_type: impl Into<String>,
        entity_labels: Vec<String>,
        occurred_at: Option<String>,
        evidence_ids: Vec<String>,
    ) -> Result<Self, DomainError> {
        let entity_labels = normalize_entity_labels(entity_labels)?;
        if entity_labels.is_empty() {
            return Err(DomainError::invalid(
                "entity_label",
                "event must reference at least one entity",
            ));
        }

        Ok(Self {
            id: required_text("event_id", id)?,
            source_scope,
            event_type: required_text("event_type", event_type)?,
            entity_labels,
            occurred_at: occurred_at
                .map(|value| required_text("occurred_at", value))
                .transpose()?,
            evidence_ids: normalize_evidence_ids(evidence_ids)?,
            confidence: ConfidenceScore::CERTAIN,
            status: FactStatus::Accepted,
            version_range: GraphVersionRange::open_from(GraphVersion::ZERO),
        })
    }

    /// Applies quality and lifecycle metadata after structural validation.
    pub fn with_metadata(
        mut self,
        confidence: ConfidenceScore,
        status: FactStatus,
        version_range: GraphVersionRange,
    ) -> Result<Self, DomainError> {
        self.confidence = confidence.validate()?;
        self.status = status;
        self.version_range = validate_version_range(version_range)?;

        Ok(self)
    }
}

/// A transactional graph mutation unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphMutationBatch {
    pub evidence: Vec<EvidenceRecord>,
    pub relations: Vec<GraphRelationRecord>,
    pub claims: Vec<ClaimRecord>,
    pub events: Vec<EventRecord>,
}

impl GraphMutationBatch {
    /// Creates a non-empty batch for a single graph transaction.
    pub fn new(evidence: Vec<EvidenceRecord>) -> Result<Self, DomainError> {
        Self::with_facts(evidence, Vec::new(), Vec::new(), Vec::new())
    }

    /// Creates a non-empty batch containing evidence and structured facts.
    pub fn with_facts(
        evidence: Vec<EvidenceRecord>,
        relations: Vec<GraphRelationRecord>,
        claims: Vec<ClaimRecord>,
        events: Vec<EventRecord>,
    ) -> Result<Self, DomainError> {
        if evidence.is_empty() && relations.is_empty() && claims.is_empty() && events.is_empty() {
            return Err(DomainError::invalid(
                "graph_facts",
                "must include at least one graph fact",
            ));
        }
        validate_unique_ids(
            "evidence_id",
            evidence.iter().map(|record| record.id.as_str()),
        )?;
        validate_unique_ids(
            "relation_id",
            relations.iter().map(|record| record.id.as_str()),
        )?;
        validate_unique_ids("claim_id", claims.iter().map(|record| record.id.as_str()))?;
        validate_unique_ids("event_id", events.iter().map(|record| record.id.as_str()))?;
        for record in &evidence {
            if let Some(span) = record.span {
                validate_span(span)?;
            }
            record.confidence.validate()?;
        }
        for record in &relations {
            validate_evidence_ids_present(&record.evidence_ids)?;
            record.confidence.validate()?;
            validate_version_range(record.version_range)?;
        }
        for record in &claims {
            validate_evidence_ids_present(&record.evidence_ids)?;
            record.confidence.validate()?;
            validate_version_range(record.version_range)?;
        }
        for record in &events {
            validate_evidence_ids_present(&record.evidence_ids)?;
            record.confidence.validate()?;
            validate_version_range(record.version_range)?;
        }

        Ok(Self {
            evidence,
            relations,
            claims,
            events,
        })
    }
}

/// Receipt returned after graph facts and mutation log entries commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitReceipt {
    pub graph_version: GraphVersion,
    pub evidence_count: usize,
    pub entity_count: usize,
    pub relation_count: usize,
    pub claim_count: usize,
    pub event_count: usize,
}

fn normalize_entity_labels(labels: Vec<String>) -> Result<Vec<String>, DomainError> {
    normalize_text_list("entity_label", labels)
}

fn normalize_evidence_ids(ids: Vec<String>) -> Result<Vec<String>, DomainError> {
    let ids = normalize_text_list("evidence_id", ids)?;
    validate_evidence_ids_present(&ids)?;

    Ok(ids)
}

fn validate_evidence_ids_present(ids: &[String]) -> Result<(), DomainError> {
    if ids.is_empty() {
        return Err(DomainError::invalid(
            "evidence_id",
            "structured facts must reference supporting evidence",
        ));
    }

    Ok(())
}

fn validate_span(span: EvidenceSpan) -> Result<EvidenceSpan, DomainError> {
    EvidenceSpan::new(
        span.start_byte,
        span.end_byte,
        span.start_line,
        span.end_line,
    )
}

fn validate_version_range(range: GraphVersionRange) -> Result<GraphVersionRange, DomainError> {
    GraphVersionRange::new(range.valid_from, range.valid_until)
}

fn normalize_text_list(
    field: &'static str,
    values: Vec<String>,
) -> Result<Vec<String>, DomainError> {
    let mut normalized = Vec::new();
    for value in values {
        let value = required_text(field, value)?;
        if !normalized.contains(&value) {
            normalized.push(value);
        }
    }

    Ok(normalized)
}

fn validate_unique_ids<'a>(
    field: &'static str,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<(), DomainError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(DomainError::invalid(
                field,
                "must be unique within a mutation batch",
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_batch_requires_graph_facts() {
        let error = GraphMutationBatch::new(Vec::new()).expect_err("empty batch should fail");

        assert_eq!(error.field, "graph_facts");
    }

    #[test]
    fn mutation_batch_rejects_duplicate_evidence_ids() {
        let scope = SourceScope::parse("repo").expect("scope should parse");
        let first = EvidenceRecord::new("ev-1", scope.clone(), "first", Vec::new())
            .expect("evidence should validate");
        let second = EvidenceRecord::new("ev-1", scope, "second", Vec::new())
            .expect("evidence should validate");

        let error =
            GraphMutationBatch::new(vec![first, second]).expect_err("duplicate ID should fail");

        assert_eq!(error.field, "evidence_id");
    }

    #[test]
    fn evidence_normalizes_duplicate_entity_labels() {
        let evidence = EvidenceRecord::new(
            "ev-1",
            SourceScope::parse("repo").expect("scope should parse"),
            "Rust owns safety guarantees",
            vec![" Rust ".to_owned(), "Rust".to_owned()],
        )
        .expect("evidence should validate");

        assert_eq!(evidence.entity_labels, ["Rust"]);
    }

    #[test]
    fn evidence_metadata_validates_span_and_confidence() {
        let span = EvidenceSpan::new(2, 8, 1, 1).expect("span should validate");
        let confidence = ConfidenceScore::from_ratio(0.875).expect("confidence should validate");
        let evidence = EvidenceRecord::new(
            "ev-1",
            SourceScope::parse("repo").expect("scope should parse"),
            "GraphRAG context packing",
            Vec::new(),
        )
        .expect("evidence should validate")
        .with_metadata(
            Some("docs/spec.md".to_owned()),
            Some(span),
            confidence,
            FactStatus::Proposed,
        )
        .expect("metadata should validate");

        assert_eq!(evidence.source_path.as_deref(), Some("docs/spec.md"));
        assert_eq!(evidence.confidence.basis_points, 8750);
        assert_eq!(evidence.status, FactStatus::Proposed);

        let invalid = EvidenceRecord::new(
            "ev-invalid",
            SourceScope::parse("repo").expect("scope should parse"),
            "GraphRAG context packing",
            Vec::new(),
        )
        .expect("evidence should validate")
        .with_metadata(
            None,
            Some(EvidenceSpan {
                start_byte: 8,
                end_byte: 2,
                start_line: 0,
                end_line: 0,
            }),
            ConfidenceScore {
                basis_points: 10_001,
            },
            FactStatus::Accepted,
        )
        .expect_err("invalid deserialized metadata should fail");
        assert_eq!(invalid.field, "evidence_span");
    }

    #[test]
    fn evidence_accepts_multimodal_extraction_metadata() {
        let evidence = EvidenceRecord::new(
            "ocr-1",
            SourceScope::parse("docs").expect("scope should parse"),
            "Detected diagram label",
            vec!["Diagram".to_owned()],
        )
        .expect("evidence should validate")
        .with_extraction_metadata(EvidenceExtractionMetadata {
            modality: crate::domain::EvidenceModality::OcrText,
            parent_evidence_id: Some("image-1".to_owned()),
            extractor: Some("ocr-worker".to_owned()),
            extractor_version: Some("1.0".to_owned()),
            ..EvidenceExtractionMetadata::text_span()
        })
        .expect("extraction metadata should validate");

        assert_eq!(evidence.extraction.modality.as_str(), "ocr_text");
        assert_eq!(
            evidence.extraction.parent_evidence_id.as_deref(),
            Some("image-1")
        );
    }

    #[test]
    fn structured_facts_validate_ids_and_version_ranges() {
        let missing_evidence =
            GraphRelationRecord::new("rel-empty", scope(), "Rust", "uses", "SQLite", Vec::new())
                .expect_err("structured facts require evidence references");
        let relation = GraphRelationRecord::new(
            "rel-1",
            scope(),
            "Rust",
            "uses",
            "SQLite",
            vec!["ev-1".to_owned()],
        )
        .expect("relation should validate");
        let claim = ClaimRecord::new(
            "claim-1",
            scope(),
            "Rust",
            "supports",
            "async service boundaries",
            vec!["ev-1".to_owned()],
        )
        .expect("claim should validate");
        let event = EventRecord::new(
            "event-1",
            scope(),
            "indexed",
            vec!["Rust".to_owned()],
            Some("2026-05-12".to_owned()),
            vec!["ev-1".to_owned()],
        )
        .expect("event should validate");
        let range_error = GraphVersionRange::new(GraphVersion::new(2), Some(GraphVersion::new(1)))
            .expect_err("reversed range should fail");
        let metadata_error = relation
            .clone()
            .with_metadata(
                ConfidenceScore {
                    basis_points: 10_001,
                },
                FactStatus::Accepted,
                GraphVersionRange {
                    valid_from: GraphVersion::new(2),
                    valid_until: Some(GraphVersion::new(1)),
                },
            )
            .expect_err("deserialized metadata should be revalidated");

        let batch =
            GraphMutationBatch::with_facts(Vec::new(), vec![relation], vec![claim], vec![event])
                .expect("structured fact batch should validate");

        assert_eq!(missing_evidence.field, "evidence_id");
        assert_eq!(range_error.field, "version_range");
        assert_eq!(metadata_error.field, "confidence");
        assert_eq!(batch.relations.len(), 1);
        assert_eq!(batch.claims.len(), 1);
        assert_eq!(batch.events.len(), 1);
    }

    #[test]
    fn parses_fact_status_wire_values() {
        assert_eq!(
            FactStatus::parse("accepted").expect("status"),
            FactStatus::Accepted
        );
        assert_eq!(
            FactStatus::parse("mystery")
                .expect_err("unknown status should fail")
                .field,
            "fact_status"
        );
    }

    fn scope() -> SourceScope {
        SourceScope::parse("repo").expect("scope should parse")
    }
}
