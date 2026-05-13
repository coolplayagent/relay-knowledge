//! GraphRAG evaluation harness for retrieval and code-impact observations.
//!
//! The harness is intentionally pure: callers run CLI/API/storage workflows,
//! then submit compact observations here so exact fact, multi-hop, temporal,
//! negative, stale-index, ambiguous-entity, and code-impact checks share one
//! scoring contract.

use std::{collections::BTreeSet, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{
    api::HybridRetrievalResponse,
    domain::{CodeRetrievalHit, RetrieverSource},
};

/// Phase 4 GraphRAG evaluation scenario family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationCaseKind {
    ExactFact,
    MultiHop,
    Temporal,
    NegativeRejection,
    StaleIndex,
    AmbiguousEntity,
    CodeImpact,
}

/// Expected behavior for one evaluation query or workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationCase {
    pub id: String,
    pub kind: EvaluationCaseKind,
    pub query: String,
    pub expected_result_ids: Vec<String>,
    pub forbidden_result_ids: Vec<String>,
    pub required_sources: Vec<RetrieverSource>,
    pub expected_stale: Option<bool>,
}

impl EvaluationCase {
    /// Validates a case and keeps expected result IDs deterministic.
    pub fn new(
        id: impl Into<String>,
        kind: EvaluationCaseKind,
        query: impl Into<String>,
    ) -> Result<Self, EvaluationError> {
        let id = required_text("case id", id.into())?;
        let query = required_text("query", query.into())?;

        Ok(Self {
            id,
            kind,
            query,
            expected_result_ids: Vec::new(),
            forbidden_result_ids: Vec::new(),
            required_sources: Vec::new(),
            expected_stale: None,
        })
    }

    /// Adds result IDs that must be present in the observation.
    pub fn requiring_results(mut self, ids: &[&str]) -> Result<Self, EvaluationError> {
        self.expected_result_ids = normalize_ids(ids)?;
        Ok(self)
    }

    /// Adds result IDs that must not appear in the observation.
    pub fn forbidding_results(mut self, ids: &[&str]) -> Result<Self, EvaluationError> {
        self.forbidden_result_ids = normalize_ids(ids)?;
        Ok(self)
    }

    /// Requires at least one observed retrieval hit from each listed source.
    pub fn requiring_sources(mut self, sources: &[RetrieverSource]) -> Self {
        self.required_sources = sources.to_vec();
        self
    }

    /// Requires the response stale flag to match the expected value.
    pub const fn expecting_stale(mut self, stale: bool) -> Self {
        self.expected_stale = Some(stale);
        self
    }
}

/// Compact observation submitted by integration tests or diagnostics commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationObservation {
    pub result_ids: Vec<String>,
    pub retriever_sources: Vec<RetrieverSource>,
    pub stale: bool,
}

impl EvaluationObservation {
    /// Captures the stable retrieval fields needed for evaluation.
    pub fn from_retrieval(response: &HybridRetrievalResponse) -> Self {
        let result_ids = response
            .results
            .iter()
            .map(|hit| hit.evidence_id.clone())
            .collect::<Vec<_>>();
        let retriever_sources = response
            .results
            .iter()
            .flat_map(|hit| hit.retriever_sources.iter().copied())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        Self {
            result_ids,
            retriever_sources,
            stale: response.metadata.stale,
        }
    }

    /// Captures code-impact hits as artifact IDs while preserving source kind.
    pub fn from_code_impact(hits: &[CodeRetrievalHit]) -> Self {
        let retriever_sources = (!hits.is_empty())
            .then_some(RetrieverSource::CodeGraph)
            .into_iter()
            .collect::<Vec<_>>();
        Self {
            result_ids: hits
                .iter()
                .map(|hit| {
                    hit.symbol_snapshot_id
                        .clone()
                        .or_else(|| hit.file_id.clone())
                        .unwrap_or_else(|| hit.path.clone())
                })
                .collect(),
            retriever_sources,
            stale: hits.iter().any(|hit| hit.stale),
        }
    }
}

/// Per-case evaluation result with concrete failure reasons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub case_id: String,
    pub kind: EvaluationCaseKind,
    pub passed: bool,
    pub missing_result_ids: Vec<String>,
    pub forbidden_result_ids: Vec<String>,
    pub missing_sources: Vec<RetrieverSource>,
    pub stale_mismatch: Option<bool>,
}

/// Aggregated report for a Phase 4 evaluation run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub passed: bool,
    pub total: usize,
    pub failed: usize,
    pub results: Vec<EvaluationResult>,
}

/// Scores one case against one observation.
pub fn evaluate_case(
    case: &EvaluationCase,
    observation: &EvaluationObservation,
) -> EvaluationResult {
    let observed_ids = observation
        .result_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let observed_sources = observation
        .retriever_sources
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let missing_result_ids = case
        .expected_result_ids
        .iter()
        .filter(|id| !observed_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let forbidden_result_ids = case
        .forbidden_result_ids
        .iter()
        .filter(|id| observed_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let missing_sources = case
        .required_sources
        .iter()
        .filter(|source| !observed_sources.contains(*source))
        .copied()
        .collect::<Vec<_>>();
    let stale_mismatch = case
        .expected_stale
        .filter(|expected| *expected != observation.stale);
    let passed = missing_result_ids.is_empty()
        && forbidden_result_ids.is_empty()
        && missing_sources.is_empty()
        && stale_mismatch.is_none();

    EvaluationResult {
        case_id: case.id.clone(),
        kind: case.kind,
        passed,
        missing_result_ids,
        forbidden_result_ids,
        missing_sources,
        stale_mismatch,
    }
}

/// Scores a suite where observations are supplied in case order.
pub fn evaluate_suite(
    cases: &[EvaluationCase],
    observations: &[EvaluationObservation],
) -> Result<EvaluationReport, EvaluationError> {
    if cases.len() != observations.len() {
        return Err(EvaluationError::MismatchedObservationCount);
    }
    let results = cases
        .iter()
        .zip(observations)
        .map(|(case, observation)| evaluate_case(case, observation))
        .collect::<Vec<_>>();
    let failed = results.iter().filter(|result| !result.passed).count();

    Ok(EvaluationReport {
        passed: failed == 0,
        total: results.len(),
        failed,
        results,
    })
}

/// Evaluation input validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationError {
    EmptyField(&'static str),
    MismatchedObservationCount,
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::MismatchedObservationCount => {
                write!(
                    formatter,
                    "evaluation case and observation counts must match"
                )
            }
        }
    }
}

impl Error for EvaluationError {}

fn required_text(field: &'static str, value: String) -> Result<String, EvaluationError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(EvaluationError::EmptyField(field));
    }

    Ok(trimmed.to_owned())
}

fn normalize_ids(ids: &[&str]) -> Result<Vec<String>, EvaluationError> {
    ids.iter()
        .map(|id| required_text("result id", (*id).to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CodeRetrievalLayer, RepositoryCodeRange};

    #[test]
    fn evaluates_phase4_case_families() {
        let cases = vec![
            EvaluationCase::new("exact", EvaluationCaseKind::ExactFact, "fact")
                .unwrap()
                .requiring_results(&["ev-1"])
                .unwrap(),
            EvaluationCase::new("multi", EvaluationCaseKind::MultiHop, "path")
                .unwrap()
                .requiring_sources(&[RetrieverSource::GraphPath]),
            EvaluationCase::new("temporal", EvaluationCaseKind::Temporal, "as_of:2026")
                .unwrap()
                .requiring_sources(&[RetrieverSource::Temporal]),
            EvaluationCase::new("negative", EvaluationCaseKind::NegativeRejection, "unknown")
                .unwrap()
                .forbidding_results(&["ev-2"])
                .unwrap(),
            EvaluationCase::new("stale", EvaluationCaseKind::StaleIndex, "stale")
                .unwrap()
                .expecting_stale(true),
            EvaluationCase::new("ambiguous", EvaluationCaseKind::AmbiguousEntity, "rust")
                .unwrap()
                .requiring_results(&["ev-rust-lang", "ev-rust-fungus"])
                .unwrap(),
            EvaluationCase::new("impact", EvaluationCaseKind::CodeImpact, "changed")
                .unwrap()
                .requiring_results(&["symbol:retry_policy"])
                .unwrap(),
        ];
        let observations = vec![
            observation(&["ev-1"], &[], false),
            observation(&["ev-path"], &[RetrieverSource::GraphPath], false),
            observation(&["ev-time"], &[RetrieverSource::Temporal], false),
            observation(&[], &[], false),
            observation(&["ev-stale"], &[], true),
            observation(&["ev-rust-lang", "ev-rust-fungus"], &[], false),
            observation(&["symbol:retry_policy"], &[], false),
        ];

        let report = evaluate_suite(&cases, &observations).expect("suite should score");

        assert!(report.passed);
        assert_eq!(report.total, 7);
    }

    #[test]
    fn reports_missing_forbidden_and_stale_failures() {
        let case = EvaluationCase::new("case", EvaluationCaseKind::ExactFact, "query")
            .unwrap()
            .requiring_results(&["wanted"])
            .unwrap()
            .forbidding_results(&["forbidden"])
            .unwrap()
            .requiring_sources(&[RetrieverSource::Vector])
            .expecting_stale(false);
        let result = evaluate_case(
            &case,
            &observation(&["forbidden"], &[RetrieverSource::Bm25], true),
        );

        assert!(!result.passed);
        assert_eq!(result.missing_result_ids, ["wanted"]);
        assert_eq!(result.forbidden_result_ids, ["forbidden"]);
        assert_eq!(result.missing_sources, [RetrieverSource::Vector]);
        assert_eq!(result.stale_mismatch, Some(false));
    }

    #[test]
    fn code_impact_observation_preserves_sources_and_stale_state() {
        let observation = EvaluationObservation::from_code_impact(&[code_hit(true)]);

        assert_eq!(observation.result_ids, ["symbol:retry_policy"]);
        assert_eq!(observation.retriever_sources, [RetrieverSource::CodeGraph]);
        assert!(observation.stale);
    }

    fn observation(
        ids: &[&str],
        sources: &[RetrieverSource],
        stale: bool,
    ) -> EvaluationObservation {
        EvaluationObservation {
            result_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
            retriever_sources: sources.to_vec(),
            stale,
        }
    }

    fn code_hit(stale: bool) -> CodeRetrievalHit {
        CodeRetrievalHit {
            repository_id: "repo".to_owned(),
            scope_id: "main".to_owned(),
            resolved_commit_sha: "abc".to_owned(),
            tree_hash: "tree".to_owned(),
            path: "src/lib.rs".to_owned(),
            language_id: "rust".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 10 },
            line_range: RepositoryCodeRange { start: 1, end: 1 },
            symbol_snapshot_id: Some("symbol:retry_policy".to_owned()),
            file_id: Some("file:src/lib.rs".to_owned()),
            retrieval_layers: vec![CodeRetrievalLayer::Impact],
            index_versions: vec!["code_graph:1".to_owned()],
            stale,
            degraded_reason: None,
            score: 1.0,
            excerpt: "fn retry_policy() {}".to_owned(),
        }
    }
}
