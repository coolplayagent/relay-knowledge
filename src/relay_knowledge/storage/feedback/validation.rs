//! Journal integrity checks before disk publication or recovery.

use std::collections::HashSet;

use crate::{
    domain::feedback::{
        FEEDBACK_MAX_VALIDATION_RUNS, FeedbackPublicationState, FeedbackRecord,
        FeedbackVerificationState, feedback_digest, prepare_payload, validate_regression,
    },
    ports::feedback_store::{
        FeedbackJournal, FeedbackStoreError, FeedbackStoreErrorKind, MAX_FEEDBACK_RECORDS,
    },
};

use super::error;

pub(super) fn validate_snapshot(snapshot: &FeedbackJournal) -> Result<(), FeedbackStoreError> {
    if snapshot.schema_version != 1 {
        return Err(invalid(
            "unsupported feedback journal schema; preserve it for recovery",
        ));
    }
    if snapshot.records.len() > MAX_FEEDBACK_RECORDS {
        return Err(error(
            FeedbackStoreErrorKind::Capacity,
            "feedback outbox reached its 1000-record budget; existing evidence was preserved",
        ));
    }
    snapshot
        .policy
        .validate()
        .map_err(|_| invalid("feedback journal contains invalid publication policy"))?;
    let mut ids = HashSet::new();
    let mut fingerprints = HashSet::new();
    let mut markers = HashSet::new();
    for record in &snapshot.records {
        if record.id.len() != 32
            || !record.id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || record.marker != format!("<!-- relay-feedback:{} -->", record.id)
            || !ids.insert(&record.id)
            || !fingerprints.insert(&record.fingerprint)
            || !markers.insert(&record.marker)
        {
            return Err(invalid(
                "feedback record identities must be bounded and unique",
            ));
        }
        validate_content_bindings(record)?;
        if record.occurrences == 0 || record.validation.runs.len() > FEEDBACK_MAX_VALIDATION_RUNS {
            return Err(invalid(
                "feedback occurrence or validation history bounds are invalid",
            ));
        }
        if record
            .report
            .fingerprint(&record.cli_version)
            .map_err(|_| invalid("feedback record report is invalid"))?
            != record.fingerprint
        {
            return Err(invalid(
                "feedback report no longer matches its local fingerprint",
            ));
        }
        if record.publication.attempts > 0
            && (record.publication.target_repository.is_none()
                || record.publication.payload.is_none())
        {
            return Err(invalid(
                "feedback send intent requires a pinned target and public payload",
            ));
        }
        if matches!(
            record.publication.state,
            FeedbackPublicationState::Publishing | FeedbackPublicationState::AwaitingReconciliation
        ) && record.publication.attempts == 0
        {
            return Err(invalid(
                "uncertain publication requires a durable send attempt",
            ));
        }
        if matches!(
            record.publication.state,
            FeedbackPublicationState::Submitted | FeedbackPublicationState::Deduplicated
        ) && (record.publication.issue.is_none()
            || record.publication.target_repository.is_none()
            || record.publication.payload.is_none())
        {
            return Err(invalid(
                "submitted feedback must retain its real issue reference",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_transition(
    previous: &FeedbackJournal,
    next: &FeedbackJournal,
) -> Result<(), FeedbackStoreError> {
    validate_snapshot(next)?;
    if next.quota_window_start_ms < previous.quota_window_start_ms
        || (next.quota_window_start_ms == previous.quota_window_start_ms
            && next.quota_attempts < previous.quota_attempts)
    {
        return Err(invalid(
            "feedback publication quota cannot decrease within its current window",
        ));
    }
    if next.quota_window_start_ms != previous.quota_window_start_ms
        && next.quota_window_start_ms < previous.quota_window_start_ms.saturating_add(86_400_000)
    {
        return Err(invalid("feedback quota windows must span at least one day"));
    }
    let old_attempts: u64 = previous
        .records
        .iter()
        .map(|record| u64::from(record.publication.attempts))
        .sum();
    let new_attempts: u64 = next
        .records
        .iter()
        .map(|record| u64::from(record.publication.attempts))
        .sum();
    let prior_quota = if next.quota_window_start_ms == previous.quota_window_start_ms {
        previous.quota_attempts
    } else {
        0
    };
    if new_attempts.saturating_sub(old_attempts)
        > u64::from(next.quota_attempts.saturating_sub(prior_quota))
    {
        return Err(invalid(
            "feedback send claims require durable quota accounting",
        ));
    }
    for old in &previous.records {
        let Some(new) = next.records.iter().find(|record| record.id == old.id) else {
            return Err(invalid(
                "feedback records cannot be discarded by an ordinary transaction",
            ));
        };
        if old.fingerprint != new.fingerprint
            || old.raw_report_digest != new.raw_report_digest
            || old.marker != new.marker
            || old.report != new.report
            || old.cli_version != new.cli_version
            || old.platform != new.platform
            || old.created_at_ms != new.created_at_ms
        {
            return Err(invalid(
                "feedback evidence and correlation identity are immutable",
            ));
        }
        if old.publication.state == FeedbackPublicationState::AwaitingReconciliation {
            validate_uncertain_record(old, new)?;
        }
        if new.occurrences < old.occurrences || new.publication.attempts < old.publication.attempts
        {
            return Err(invalid(
                "feedback occurrence and publication-attempt counters cannot decrease",
            ));
        }
        if (old.publication.attempts > 0 || old.publication.issue.is_some())
            && (old.publication.target_repository != new.publication.target_repository
                || old.publication.payload != new.publication.payload)
        {
            return Err(invalid(
                "published feedback target and payload are immutable",
            ));
        }
        if let Some(issue) = &old.publication.issue {
            if new.publication.issue.as_ref().is_none_or(|new_issue| {
                issue.number != new_issue.number || issue.url != new_issue.url
            }) {
                return Err(invalid(
                    "feedback cannot lose or replace its published issue identity",
                ));
            }
        }
        if !new.validation.runs.starts_with(&old.validation.runs) {
            return Err(invalid("feedback regression evidence is append-only"));
        }
    }
    Ok(())
}

pub(super) fn validate_recovery(
    origin: &FeedbackJournal,
    next: &FeedbackJournal,
) -> Result<(), FeedbackStoreError> {
    for old in &origin.records {
        if matches!(
            old.publication.state,
            FeedbackPublicationState::Publishing | FeedbackPublicationState::AwaitingReconciliation
        ) {
            let new = next
                .records
                .iter()
                .find(|record| record.id == old.id)
                .ok_or_else(|| invalid("recovery cannot discard uncertain feedback"))?;
            validate_uncertain_record(old, new)?;
        }
    }
    Ok(())
}

fn validate_uncertain_record(
    old: &FeedbackRecord,
    new: &FeedbackRecord,
) -> Result<(), FeedbackStoreError> {
    if old.publication.attempts != new.publication.attempts
        || !matches!(
            new.publication.state,
            FeedbackPublicationState::Publishing
                | FeedbackPublicationState::AwaitingReconciliation
                | FeedbackPublicationState::Submitted
                | FeedbackPublicationState::Deduplicated
        )
    {
        return Err(invalid(
            "uncertain feedback may only reconcile its original send claim",
        ));
    }
    Ok(())
}

fn validate_content_bindings(record: &FeedbackRecord) -> Result<(), FeedbackStoreError> {
    let raw = serde_json::to_vec(&record.report)
        .map_err(|_| invalid("feedback raw report cannot be encoded"))?;
    if feedback_digest(&raw) != record.raw_report_digest {
        return Err(invalid(
            "feedback raw evidence no longer matches its content binding",
        ));
    }
    if let Some(payload) = &record.publication.payload {
        let expected = prepare_payload(
            &record.report,
            &record.cli_version,
            &record.platform,
            &record.marker,
        )
        .map_err(|_| invalid("feedback public payload no longer satisfies disclosure policy"))?;
        if payload != &expected {
            return Err(invalid(
                "feedback public payload no longer matches its content binding",
            ));
        }
    }
    if matches!(
        record.validation.state,
        FeedbackVerificationState::VerifiedFixed | FeedbackVerificationState::Regressed
    ) {
        let fix = record
            .validation
            .fix
            .as_ref()
            .ok_or_else(|| invalid("feedback verdict requires a linked fix"))?;
        let run = record
            .validation
            .runs
            .last()
            .ok_or_else(|| invalid("feedback verdict requires original scenario evidence"))?;
        let expected =
            validate_regression(&record.report, fix, run.input.clone(), run.recorded_at_ms)
                .map_err(|_| {
                    invalid("feedback verdict does not bind its original scenario and linked fix")
                })?;
        if &expected != run
            || (record.validation.state == FeedbackVerificationState::VerifiedFixed) != run.passed
        {
            return Err(invalid(
                "feedback verdict contradicts its recorded scenario result",
            ));
        }
    }
    Ok(())
}

fn invalid(message: &str) -> FeedbackStoreError {
    error(FeedbackStoreErrorKind::InvalidData, message)
}
