//! Versioned, bounded feedback input and durable lifecycle records.

use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

pub const FEEDBACK_SCHEMA_VERSION: u32 = 1;
pub const FEEDBACK_MAX_REPORT_BYTES: usize = 65_536;
pub const FEEDBACK_MAX_TEXT_BYTES: usize = 4_096;
pub const FEEDBACK_MAX_EVIDENCE_COUNT: usize = 16;
pub const FEEDBACK_MAX_VALIDATION_RUNS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackKind {
    Bug,
    MissingCapability,
    PoorResult,
    WorkflowFriction,
    Performance,
    Documentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackMode {
    LocalOnly,
    AutoSubmit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackPolicy {
    pub schema_version: u32,
    pub mode: FeedbackMode,
    pub target_repository: Option<String>,
    pub allowed_kinds: Vec<FeedbackKind>,
    pub daily_quota: u32,
    /// Independently authorized local runner identity; publication does not enable validation.
    pub validation_runner: Option<String>,
}

impl Default for FeedbackPolicy {
    fn default() -> Self {
        Self {
            schema_version: FEEDBACK_SCHEMA_VERSION,
            mode: FeedbackMode::LocalOnly,
            target_repository: None,
            allowed_kinds: Vec::new(),
            daily_quota: 5,
            validation_runner: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackObservationOrigin {
    AgentObservation,
    UserExperience,
    CliFact,
    Hypothesis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackObservation {
    pub origin: FeedbackObservationOrigin,
    pub text: String,
}

/// Raw evidence is retained locally and is never copied into a public payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackEvidence {
    pub label: String,
    pub content: String,
}

/// An inert scenario and its exact expected result; neither is a shell command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackReproduction {
    pub scenario: String,
    pub expected: String,
}

/// Caller-supplied diagnostic baseline, retained locally with the raw report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackDiagnostics {
    pub command: Option<String>,
    pub exit_status: Option<i32>,
    pub freshness: Option<String>,
    pub content_integrity: Option<String>,
    pub environment: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub steps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackReport {
    pub schema_version: u32,
    pub kind: FeedbackKind,
    pub intent: String,
    pub expected: String,
    pub actual: String,
    pub impact: String,
    #[serde(default)]
    pub observations: Vec<FeedbackObservation>,
    #[serde(default)]
    pub evidence: Vec<FeedbackEvidence>,
    pub reproduction: Option<FeedbackReproduction>,
    pub trace_id: Option<String>,
    pub request_id: Option<String>,
    pub diagnostics: Option<FeedbackDiagnostics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackPublicationState {
    Draft,
    Pending,
    Publishing,
    Submitted,
    Deduplicated,
    RetryableFailed,
    Blocked,
    EvidenceInsufficient,
    AwaitingReconciliation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackPayload {
    pub title: String,
    pub body: String,
    /// Identity of sanitized public text, excluding the random recovery nonce.
    pub dedup_marker: String,
    /// Binds only the public title/body, never the raw private report.
    pub digest: String,
    /// Fixed public omission categories, never caller-supplied evidence labels.
    pub omitted_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackIssue {
    pub number: u64,
    pub url: String,
    pub state: String,
    /// SHA-256 of the public body actually observed in the provider response.
    pub body_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackPublication {
    pub state: FeedbackPublicationState,
    pub reason: Option<String>,
    pub target_repository: Option<String>,
    pub payload: Option<FeedbackPayload>,
    pub issue: Option<FeedbackIssue>,
    pub attempts: u32,
    pub next_attempt_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackVerificationState {
    AwaitingFix,
    AwaitingValidation,
    VerifiedFixed,
    Regressed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackFix {
    pub reference: String,
    pub target_version: String,
}

/// Evidence from a separately authorized runner; a boolean verdict is not input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackValidation {
    pub runner: String,
    pub scenario_digest: String,
    pub actual: String,
    pub version: String,
    pub environment: String,
    pub run_id: String,
    pub elapsed_ms: Option<u64>,
    pub steps: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackValidationResult {
    pub input: FeedbackValidation,
    pub passed: bool,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackValidationStatus {
    pub state: FeedbackVerificationState,
    pub fix: Option<FeedbackFix>,
    pub runs: Vec<FeedbackValidationResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackRecord {
    pub id: String,
    /// Local-only content identity; never use it as a public correlation marker.
    pub fingerprint: String,
    /// Local content binding for the entire original report, including private evidence.
    pub raw_report_digest: String,
    pub marker: String,
    pub report: FeedbackReport,
    pub cli_version: String,
    pub platform: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub occurrences: u64,
    pub publication: FeedbackPublication,
    pub validation: FeedbackValidationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackError(pub String);

impl fmt::Display for FeedbackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for FeedbackError {}
