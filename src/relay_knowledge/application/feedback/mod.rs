//! Durable software feedback, independent from graph mutation and proposals.

mod publication;
mod validation;

#[cfg(test)]
mod mod_tests;
#[cfg(test)]
mod test_support;

use std::sync::Arc;

use crate::{
    api::{ApiError, RequestContext},
    clock::system_now_millis,
    domain::feedback::*,
    ports::{
        feedback::FeedbackProvider,
        feedback_store::{
            FeedbackStore, FeedbackStoreError, FeedbackStoreErrorKind, FeedbackTransaction,
        },
    },
};

use super::RelayKnowledgeService;

/// Shared CLI/Web workflow with explicitly injected persistence and publisher.
#[derive(Clone)]
pub struct FeedbackService {
    store: Arc<dyn FeedbackStore>,
    provider: Arc<dyn FeedbackProvider>,
    platform: String,
}

impl RelayKnowledgeService {
    /// Attaches operational feedback adapters at the outer composition boundary.
    pub fn with_feedback(mut self, feedback: FeedbackService) -> Self {
        self.feedback = Some(feedback);
        self
    }

    /// Gets the configured shared workflow without opening graph storage.
    pub fn feedback_service(&self) -> Result<&FeedbackService, ApiError> {
        self.feedback
            .as_ref()
            .ok_or_else(|| ApiError::storage_unavailable("feedback adapters are unavailable"))
    }
}

impl FeedbackService {
    /// Injects storage, provider, and bootstrap-captured platform metadata without I/O.
    pub fn new(
        store: Arc<dyn FeedbackStore>,
        provider: Arc<dyn FeedbackProvider>,
        platform: String,
    ) -> Self {
        Self {
            store,
            provider,
            platform,
        }
    }

    /// Persists explicitly supplied publication authority without submitting drafts.
    pub async fn configure(&self, mut policy: FeedbackPolicy) -> Result<FeedbackPolicy, ApiError> {
        policy
            .validate()
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        if let Some(repository) = policy.target_repository.as_mut() {
            repository.make_ascii_lowercase();
        }
        let mut transaction = self.store.begin().await.map_err(store_error)?;
        transaction.snapshot_mut().policy = policy.clone();
        transaction.commit().await.map_err(store_error)?;
        Ok(policy)
    }

    /// Preserves a bounded report before any network operation, merging repeat observations.
    pub async fn report(
        &self,
        mut report: FeedbackReport,
        context: &RequestContext,
    ) -> Result<FeedbackRecord, ApiError> {
        report
            .validate()
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        let fingerprint = report
            .fingerprint(env!("CARGO_PKG_VERSION"))
            .map_err(|error| ApiError::invalid_argument(error.to_string()))?;
        report
            .trace_id
            .get_or_insert_with(|| context.trace_id.clone());
        report
            .request_id
            .get_or_insert_with(|| context.request_id.clone());
        let now = now_ms()?;
        let mut transaction = self.store.begin().await.map_err(store_error)?;
        let journal = transaction.snapshot_mut();
        let index = if let Some(index) = journal
            .records
            .iter()
            .position(|record| record.fingerprint == fingerprint)
        {
            let record = &mut journal.records[index];
            record.occurrences = record.occurrences.saturating_add(1);
            record.updated_at_ms = now;
            index
        } else {
            let mut nonce = [0u8; 16];
            getrandom::getrandom(&mut nonce)
                .map_err(|_| ApiError::internal("feedback random identity unavailable"))?;
            let id = nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let marker = format!("<!-- relay-feedback:{id} -->");
            let platform = self.platform.clone();
            let payload = prepare_payload(&report, env!("CARGO_PKG_VERSION"), &platform, &marker);
            let (state, reason, payload) = match payload {
                Ok(payload) => (
                    if journal.policy.mode == FeedbackMode::AutoSubmit {
                        FeedbackPublicationState::Pending
                    } else {
                        FeedbackPublicationState::Draft
                    },
                    None,
                    Some(payload),
                ),
                Err(error) => (
                    FeedbackPublicationState::EvidenceInsufficient,
                    Some(error.to_string()),
                    None,
                ),
            };
            journal.records.push(FeedbackRecord {
                id,
                fingerprint,
                marker,
                raw_report_digest: feedback_digest(
                    &serde_json::to_vec(&report)
                        .map_err(|error| ApiError::internal(error.to_string()))?,
                ),
                report,
                cli_version: env!("CARGO_PKG_VERSION").into(),
                platform,
                created_at_ms: now,
                updated_at_ms: now,
                occurrences: 1,
                publication: FeedbackPublication {
                    state,
                    reason,
                    target_repository: None,
                    payload,
                    issue: None,
                    attempts: 0,
                    next_attempt_at_ms: 0,
                },
                validation: FeedbackValidationStatus {
                    state: FeedbackVerificationState::AwaitingFix,
                    fix: None,
                    runs: Vec::new(),
                },
            });
            journal.records.len() - 1
        };
        let record = journal.records[index].clone();
        let auto_submit = journal.policy.mode == FeedbackMode::AutoSubmit;
        transaction.commit().await.map_err(store_error)?;
        drop(transaction);
        if auto_submit {
            // A failed acknowledgement may follow a committed send or even a
            // committed success. Reopen authority instead of returning a stale draft.
            match self.submit(&record.id).await {
                Ok(submitted) => Ok(submitted),
                Err(_) => {
                    let recovered = self.store.begin().await.map_err(|_| ApiError::storage_unavailable(format!(
                        "feedback {} was saved; publication status is unavailable; run feedback status before retrying", record.id
                    )))?;
                    let index = record_index(recovered.as_ref(), &record.id)?;
                    Ok(recovered.snapshot().records[index].clone())
                }
            }
        } else {
            Ok(record)
        }
    }

    /// Reads bounded status; private report bytes never appear in this view.
    pub async fn status(&self, id: Option<&str>) -> Result<serde_json::Value, ApiError> {
        let transaction = self.store.begin().await.map_err(store_error)?;
        let journal = transaction.snapshot();
        if let Some(id) = id {
            let index = record_index(transaction.as_ref(), id)?;
            return Ok(serde_json::json!({"feedback":public_record(&journal.records[index])}));
        }
        Ok(
            serde_json::json!({"policy":journal.policy,"feedback":journal.records.iter().map(public_record).collect::<Vec<_>>(),"quota_attempts":journal.quota_attempts,"quota_window_start_ms":journal.quota_window_start_ms}),
        )
    }

    /// Returns the exact immutable publication payload and local evidence bindings.
    pub async fn preview(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        let transaction = self.store.begin().await.map_err(store_error)?;
        let index = record_index(transaction.as_ref(), id)?;
        let record = &transaction.snapshot().records[index];
        Ok(
            serde_json::json!({"feedback":public_record(record),"payload":record.publication.payload,"configured_target":transaction.snapshot().policy.target_repository}),
        )
    }
}

/// Public status omits private narrative, diagnostic input and raw evidence bytes.
pub fn public_record(record: &FeedbackRecord) -> serde_json::Value {
    serde_json::json!({
        "id":record.id,"kind":record.report.kind,"occurrences":record.occurrences,
        "cli_version":record.cli_version,"platform":record.platform,
        "trace_id":record.report.trace_id,"request_id":record.report.request_id,
        "created_at_ms":record.created_at_ms,"updated_at_ms":record.updated_at_ms,
        "publication":record.publication,"validation":record.validation,
        "evidence":{"raw_digest":record.raw_report_digest,"count":record.report.evidence.len(),"local_only":true},
        "scenario_digest":record.report.reproduction.as_ref().map(|scenario| scenario.digest()),
    })
}

fn record_index(transaction: &dyn FeedbackTransaction, id: &str) -> Result<usize, ApiError> {
    transaction
        .snapshot()
        .records
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| ApiError::invalid_argument("feedback ID was not found"))
}

fn now_ms() -> Result<u64, ApiError> {
    system_now_millis().map_err(|error| ApiError::internal(error.to_string()))
}

fn store_error(error: FeedbackStoreError) -> ApiError {
    match error.kind {
        FeedbackStoreErrorKind::Busy => ApiError::qos_rejected(error.message),
        FeedbackStoreErrorKind::Capacity
        | FeedbackStoreErrorKind::InvalidData
        | FeedbackStoreErrorKind::Io => ApiError::storage_unavailable(error.message),
    }
}
