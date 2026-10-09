//! Serial, crash-recoverable publication; uncertain sends are never repeated.

use crate::{
    api::ApiError,
    domain::feedback::*,
    ports::{
        feedback::{FeedbackProviderError, FeedbackProviderErrorKind},
        feedback_store::FeedbackTransaction,
    },
};

use super::{FeedbackService, now_ms, record_index, store_error};

const DAY_MS: u64 = 86_400_000;
const MAX_ATTEMPTS: u32 = 5;

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "publication_recovery_tests.rs"]
mod recovery_tests;

impl FeedbackService {
    /// Runs one bounded attempt or reconciles an uncertain attempt using its nonce.
    pub async fn submit(&self, id: &str) -> Result<FeedbackRecord, ApiError> {
        let mut transaction = self.store.begin().await.map_err(store_error)?;
        let index = record_index(transaction.as_ref(), id)?;
        let mut record = transaction.snapshot().records[index].clone();
        if record.publication.issue.is_some() || record.publication.payload.is_none() {
            return Ok(record);
        }
        let policy = transaction.snapshot().policy.clone();
        let now = now_ms()?;
        let uncertain = matches!(
            record.publication.state,
            FeedbackPublicationState::Publishing | FeedbackPublicationState::AwaitingReconciliation
        );
        if policy.mode != FeedbackMode::AutoSubmit {
            record.publication.reason =
                Some("local-only policy; configure auto-submit before publication".into());
            return save(transaction.as_mut(), index, record).await;
        }
        if !policy.allowed_kinds.contains(&record.report.kind) {
            if !uncertain {
                record.publication.state = FeedbackPublicationState::Blocked;
            }
            record.publication.reason =
                Some("feedback kind is not authorized by publication policy".into());
            return save(transaction.as_mut(), index, record).await;
        }
        let target = policy
            .target_repository
            .ok_or_else(|| ApiError::invalid_argument("publication target is not configured"))?;
        if record
            .publication
            .target_repository
            .as_ref()
            .is_some_and(|pinned| pinned != &target)
        {
            record.publication.reason = Some("publication is pinned to its original repository; restore matching policy to reconcile".into());
            return save(transaction.as_mut(), index, record).await;
        }
        if now < record.publication.next_attempt_at_ms {
            return Ok(record);
        }
        // Safe first attempts may attach to an identical already-public report.
        // Recovery uses only this attempt's random nonce, never a private hash.
        let lookup_marker = if uncertain {
            &record.marker
        } else {
            &record
                .publication
                .payload
                .as_ref()
                .ok_or_else(|| ApiError::internal("public payload is unavailable"))?
                .dedup_marker
        };
        match self.provider.find_marker(&target, lookup_marker).await {
            Ok(Some(issue)) => {
                record.publication.issue = Some(issue);
                record.publication.target_repository = Some(target);
                record.publication.state = FeedbackPublicationState::Deduplicated;
                record.publication.reason = None;
                return save(transaction.as_mut(), index, record).await;
            }
            Ok(None) if uncertain => {
                record.publication.state = FeedbackPublicationState::AwaitingReconciliation;
                record.publication.reason = Some("creation outcome remains unknown; retry only rechecks the remote marker and never sends another POST".into());
                record.publication.next_attempt_at_ms = now.saturating_add(60_000);
                return save(transaction.as_mut(), index, record).await;
            }
            Err(error) => {
                apply_error(&mut record, error, now, uncertain);
                return save(transaction.as_mut(), index, record).await;
            }
            Ok(None) => {}
        }
        if record.publication.attempts >= MAX_ATTEMPTS {
            record.publication.state = FeedbackPublicationState::Blocked;
            record.publication.reason = Some("publication attempt budget exhausted".into());
            return save(transaction.as_mut(), index, record).await;
        }
        let journal = transaction.snapshot_mut();
        if now >= journal.quota_window_start_ms.saturating_add(DAY_MS) {
            journal.quota_window_start_ms = now;
            journal.quota_attempts = 0;
        }
        if journal.quota_attempts >= policy.daily_quota {
            record.publication.state = FeedbackPublicationState::Blocked;
            record.publication.reason =
                Some("publication quota exhausted; retry after the quota window".into());
            record.publication.next_attempt_at_ms =
                journal.quota_window_start_ms.saturating_add(DAY_MS);
            return save(transaction.as_mut(), index, record).await;
        }
        journal.quota_attempts += 1;
        record.publication.attempts += 1;
        record.publication.target_repository = Some(target.clone());
        record.publication.state = FeedbackPublicationState::Publishing;
        record.publication.reason =
            Some("send intent persisted; interruption requires remote reconciliation".into());
        record = save(transaction.as_mut(), index, record).await?;
        // The durable send intent and quota commit MUST precede any remote mutation.
        let payload = record
            .publication
            .payload
            .as_ref()
            .ok_or_else(|| ApiError::internal("public payload is unavailable"))?;
        match self
            .provider
            .create_issue(&target, &payload.title, &payload.body)
            .await
        {
            Ok(issue) => {
                record.publication.issue = Some(issue);
                record.publication.state = FeedbackPublicationState::Submitted;
                record.publication.reason = None;
                record.publication.next_attempt_at_ms = 0;
            }
            Err(error) => apply_error(&mut record, error, now, false),
        }
        save(transaction.as_mut(), index, record).await
    }

    /// Reads remote issue state without interpreting closure as a successful fix.
    pub async fn track(&self, id: &str) -> Result<FeedbackRecord, ApiError> {
        let mut transaction = self.store.begin().await.map_err(store_error)?;
        let index = record_index(transaction.as_ref(), id)?;
        let mut record = transaction.snapshot().records[index].clone();
        let issue = record.publication.issue.as_ref().ok_or_else(|| {
            ApiError::invalid_argument("feedback has no published issue; use retry to reconcile")
        })?;
        let target = record
            .publication
            .target_repository
            .as_deref()
            .ok_or_else(|| ApiError::internal("published issue has no pinned repository"))?;
        let updated = self
            .provider
            .read_issue(target, issue.number)
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        record.publication.issue = Some(updated);
        save(transaction.as_mut(), index, record).await
    }
}

fn apply_error(
    record: &mut FeedbackRecord,
    error: FeedbackProviderError,
    now: u64,
    already_uncertain: bool,
) {
    record.publication.state =
        if already_uncertain || error.kind == FeedbackProviderErrorKind::Ambiguous {
            FeedbackPublicationState::AwaitingReconciliation
        } else if error.kind == FeedbackProviderErrorKind::Retryable {
            FeedbackPublicationState::RetryableFailed
        } else {
            FeedbackPublicationState::Blocked
        };
    record.publication.reason = Some(error.message);
    record.publication.next_attempt_at_ms =
        now.saturating_add(30_000u64.saturating_mul(1u64 << record.publication.attempts.min(5)));
}

pub(super) async fn save(
    transaction: &mut dyn FeedbackTransaction,
    index: usize,
    mut record: FeedbackRecord,
) -> Result<FeedbackRecord, ApiError> {
    record.updated_at_ms = now_ms()?;
    transaction.snapshot_mut().records[index] = record.clone();
    transaction.commit().await.map_err(store_error)?;
    Ok(record)
}
