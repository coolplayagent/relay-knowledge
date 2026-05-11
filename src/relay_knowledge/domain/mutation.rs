use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{DomainError, GraphVersion, SourceScope, error::required_text};

/// Evidence accepted by the graph mutation pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub source_scope: SourceScope,
    pub content: String,
    pub entity_labels: Vec<String>,
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
            content: required_text("evidence_content", content)?,
            entity_labels: normalized_labels,
        })
    }
}

/// A transactional graph mutation unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphMutationBatch {
    pub evidence: Vec<EvidenceRecord>,
}

impl GraphMutationBatch {
    /// Creates a non-empty batch for a single graph transaction.
    pub fn new(evidence: Vec<EvidenceRecord>) -> Result<Self, DomainError> {
        if evidence.is_empty() {
            return Err(DomainError::invalid(
                "evidence",
                "must include at least one record",
            ));
        }
        let mut evidence_ids = BTreeSet::new();
        for record in &evidence {
            if !evidence_ids.insert(record.id.as_str()) {
                return Err(DomainError::invalid(
                    "evidence_id",
                    "must be unique within a mutation batch",
                ));
            }
        }

        Ok(Self { evidence })
    }
}

/// Receipt returned after graph facts and mutation log entries commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitReceipt {
    pub graph_version: GraphVersion,
    pub evidence_count: usize,
    pub entity_count: usize,
}

fn normalize_entity_labels(labels: Vec<String>) -> Result<Vec<String>, DomainError> {
    let mut normalized = Vec::new();
    for label in labels {
        let label = required_text("entity_label", label)?;
        if !normalized.contains(&label) {
            normalized.push(label);
        }
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_batch_requires_evidence() {
        let error = GraphMutationBatch::new(Vec::new()).expect_err("empty batch should fail");

        assert_eq!(error.field, "evidence");
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
}
