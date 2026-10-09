use std::sync::atomic::Ordering;

use super::*;
use crate::application::feedback::test_support::*;
use crate::ports::feedback_store::FeedbackStore;

#[tokio::test]
async fn definitive_retry_is_bounded_by_the_durable_attempt_budget() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    fixture
        .provider
        .create_results
        .lock()
        .expect("responses")
        .extend((0..MAX_ATTEMPTS).map(|_| Err(failure(FeedbackProviderErrorKind::Retryable))));
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("first attempt saved");
    for expected in 2..=MAX_ATTEMPTS {
        fixture.clear_backoff(&record.id).await;
        let retried = fixture
            .service
            .submit(&record.id)
            .await
            .expect("bounded retry");
        assert_eq!(
            retried.publication.state,
            FeedbackPublicationState::RetryableFailed
        );
        assert_eq!(retried.publication.attempts, expected);
    }
    fixture.clear_backoff(&record.id).await;
    let exhausted = fixture
        .service
        .submit(&record.id)
        .await
        .expect("attempt budget saved");
    assert_eq!(
        exhausted.publication.state,
        FeedbackPublicationState::Blocked
    );
    assert!(
        exhausted
            .publication
            .reason
            .expect("reason")
            .contains("attempt budget")
    );
    assert_eq!(
        fixture.provider.creates.load(Ordering::Relaxed),
        MAX_ATTEMPTS as usize
    );
}

#[tokio::test]
async fn elapsed_quota_window_resets_before_the_next_durable_send_claim() {
    let fixture = Fixture::new().await;
    let mut limited = policy();
    limited.daily_quota = 1;
    fixture.service.configure(limited).await.expect("authorize");
    {
        let mut transaction = fixture.store.begin().await.expect("initialize old quota");
        let journal = transaction.snapshot_mut();
        journal.quota_window_start_ms = now_ms().expect("clock").saturating_sub(DAY_MS + 1000);
        journal.quota_attempts = 1;
        transaction.commit().await.expect("old window snapshot");
    }
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("new window admits");
    assert_eq!(
        record.publication.state,
        FeedbackPublicationState::Submitted
    );
    assert_eq!(
        fixture.service.status(None).await.expect("quota")["quota_attempts"],
        1
    );
}

#[tokio::test]
async fn remote_marker_match_deduplicates_without_charging_create_quota() {
    let fixture = Fixture::new().await;
    fixture
        .provider
        .found
        .lock()
        .expect("responses")
        .push_back(Ok(Some(remote_issue("closed"))));
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("remote dedup");
    assert_eq!(
        record.publication.state,
        FeedbackPublicationState::Deduplicated
    );
    assert_eq!(
        record.publication.issue.expect("existing issue").state,
        "closed"
    );
    assert_eq!(
        record.validation.state,
        FeedbackVerificationState::AwaitingFix
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 0);
    assert_eq!(
        fixture.service.status(None).await.expect("quota")["quota_attempts"],
        0
    );
}

#[tokio::test]
async fn ambiguous_send_retries_reconciliation_without_recreating_the_issue() {
    let fixture = Fixture::new().await;
    fixture
        .provider
        .create_results
        .lock()
        .expect("responses")
        .push_back(Err(failure(FeedbackProviderErrorKind::Ambiguous)));
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("preserved report");
    assert_eq!(
        record.publication.state,
        FeedbackPublicationState::AwaitingReconciliation
    );
    let immediately = fixture
        .service
        .submit(&record.id)
        .await
        .expect("bounded backoff");
    assert_eq!(immediately.publication.attempts, 1);
    assert_eq!(fixture.provider.searches.load(Ordering::Relaxed), 1);
    fixture.clear_backoff(&record.id).await;
    let still_unknown = fixture
        .service
        .submit(&record.id)
        .await
        .expect("read-only recovery");
    assert_eq!(
        still_unknown.publication.state,
        FeedbackPublicationState::AwaitingReconciliation
    );
    fixture.clear_backoff(&record.id).await;
    fixture
        .provider
        .found
        .lock()
        .expect("responses")
        .push_back(Ok(Some(remote_issue("open"))));
    let recovered = fixture
        .service
        .submit(&record.id)
        .await
        .expect("marker recovered");
    assert_eq!(
        recovered.publication.state,
        FeedbackPublicationState::Deduplicated
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn cancellation_and_competing_submit_preserve_send_intent_before_network_mutation() {
    let fixture = Fixture::new().await;
    let draft = fixture
        .service
        .report(report(), &context())
        .await
        .expect("draft");
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    fixture
        .provider
        .wait_on_create
        .store(true, Ordering::Relaxed);
    let service = fixture.service.clone();
    let id = draft.id.clone();
    let first = tokio::spawn(async move { service.submit(&id).await });
    fixture.provider.create_started.notified().await;
    assert!(
        fixture.service.submit(&draft.id).await.is_err(),
        "OS transaction lock rejects overlapping writers"
    );
    first.abort();
    assert!(first.await.expect_err("cancelled").is_cancelled());
    let reopened = FeedbackService::new(
        fixture.store.clone(),
        fixture.provider.clone(),
        "test-host".into(),
    );
    let persisted = reopened
        .status(Some(&draft.id))
        .await
        .expect("reopened journal");
    assert_eq!(persisted["feedback"]["publication"]["state"], "publishing");
    assert_eq!(persisted["feedback"]["publication"]["attempts"], 1);
    let recovered = reopened
        .submit(&draft.id)
        .await
        .expect("reconcile uncertain send");
    assert_eq!(
        recovered.publication.state,
        FeedbackPublicationState::AwaitingReconciliation
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
    assert_eq!(
        reopened.status(None).await.expect("quota persisted")["quota_attempts"],
        1
    );
}

#[tokio::test]
async fn disabling_and_restoring_policy_never_erases_uncertain_send_or_target_pin() {
    let fixture = Fixture::new().await;
    fixture
        .provider
        .create_results
        .lock()
        .expect("responses")
        .push_back(Err(failure(FeedbackProviderErrorKind::Ambiguous)));
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("saved");
    fixture.clear_backoff(&record.id).await;
    fixture
        .service
        .configure(FeedbackPolicy::default())
        .await
        .expect("disable");
    assert_eq!(
        fixture
            .service
            .submit(&record.id)
            .await
            .expect("local-only")
            .publication
            .state,
        FeedbackPublicationState::AwaitingReconciliation
    );
    let mut other = policy();
    other.target_repository = Some("different/target".into());
    fixture
        .service
        .configure(other)
        .await
        .expect("new target applies to new reports");
    let pinned = fixture
        .service
        .submit(&record.id)
        .await
        .expect("old target remains pinned");
    assert_eq!(
        pinned.publication.target_repository.as_deref(),
        Some("acme/project")
    );
    assert!(
        pinned
            .publication
            .reason
            .as_deref()
            .expect("reason")
            .contains("pinned")
    );
    let mut disabled_kind = policy();
    disabled_kind.allowed_kinds = vec![FeedbackKind::Bug];
    fixture
        .service
        .configure(disabled_kind)
        .await
        .expect("deny kind");
    assert_eq!(
        fixture
            .service
            .submit(&record.id)
            .await
            .expect("denied")
            .publication
            .state,
        FeedbackPublicationState::AwaitingReconciliation
    );
    fixture
        .service
        .configure(policy())
        .await
        .expect("restore authority");
    fixture
        .provider
        .found
        .lock()
        .expect("responses")
        .push_back(Ok(Some(remote_issue("open"))));
    assert_eq!(
        fixture
            .service
            .submit(&record.id)
            .await
            .expect("recover")
            .publication
            .state,
        FeedbackPublicationState::Deduplicated
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn definitive_rejection_and_read_failure_have_deterministic_saved_states() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    fixture
        .provider
        .found
        .lock()
        .expect("responses")
        .push_back(Err(failure(FeedbackProviderErrorKind::Retryable)));
    let failed_read = fixture
        .service
        .report(report(), &context())
        .await
        .expect("original task report preserved offline");
    assert_eq!(
        failed_read.publication.state,
        FeedbackPublicationState::RetryableFailed
    );
    assert_eq!(failed_read.publication.attempts, 0);
    fixture.clear_backoff(&failed_read.id).await;
    fixture
        .provider
        .create_results
        .lock()
        .expect("responses")
        .push_back(Err(failure(FeedbackProviderErrorKind::Rejected)));
    assert_eq!(
        fixture
            .service
            .submit(&failed_read.id)
            .await
            .expect("permission failure preserved")
            .publication
            .state,
        FeedbackPublicationState::Blocked
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
    fixture.clear_backoff(&failed_read.id).await;
    assert_eq!(
        fixture
            .service
            .submit(&failed_read.id)
            .await
            .expect("credential repaired; safe retry")
            .publication
            .state,
        FeedbackPublicationState::Submitted
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn quota_and_kind_authorization_bound_mutation_and_preserve_reports() {
    let fixture = Fixture::new().await;
    let mut limited = policy();
    limited.daily_quota = 1;
    fixture
        .service
        .configure(limited)
        .await
        .expect("quota policy");
    fixture
        .service
        .report(report(), &context())
        .await
        .expect("first issue");
    let mut another = report();
    another.actual = "A different observable friction".into();
    let exhausted = fixture
        .service
        .report(another, &context())
        .await
        .expect("quota report retained");
    assert_eq!(
        exhausted.publication.state,
        FeedbackPublicationState::Blocked
    );
    assert_eq!(exhausted.publication.attempts, 0);
    assert!(
        exhausted
            .publication
            .reason
            .expect("reason")
            .contains("quota")
    );
    let mut denied = report();
    denied.kind = FeedbackKind::Bug;
    assert_eq!(
        fixture
            .service
            .report(denied, &context())
            .await
            .expect("disallowed kind retained")
            .publication
            .state,
        FeedbackPublicationState::Blocked
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn closed_remote_issue_is_read_only_and_does_not_claim_a_fix() {
    let fixture = Fixture::new().await;
    let draft = fixture
        .service
        .report(report(), &context())
        .await
        .expect("draft");
    assert!(fixture.service.track(&draft.id).await.is_err());
    fixture
        .service
        .configure(policy())
        .await
        .expect("authorize");
    fixture.service.submit(&draft.id).await.expect("publish");
    *fixture.provider.remote_state.lock().expect("remote state") = "closed".into();
    let tracked = fixture.service.track(&draft.id).await.expect("read state");
    assert_eq!(tracked.publication.issue.expect("issue").state, "closed");
    assert_eq!(
        tracked.validation.state,
        FeedbackVerificationState::AwaitingFix
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
    assert_eq!(fixture.provider.reads.load(Ordering::Relaxed), 1);
}
#[tokio::test]
async fn provider_rate_limit_deadline_survives_storage_and_blocks_early_retry() {
    for creating in [false, true] {
        let fixture = Fixture::new().await;
        fixture.service.configure(policy()).await.unwrap();
        let deadline = now_ms().unwrap() + 3_600_000;
        let mut error = failure(FeedbackProviderErrorKind::Retryable);
        error.retry_not_before_ms = Some(deadline);
        if creating {
            fixture
                .provider
                .create_results
                .lock()
                .unwrap()
                .push_back(Err(error));
        } else {
            fixture.provider.found.lock().unwrap().push_back(Err(error));
        }
        let record = fixture.service.report(report(), &context()).await.unwrap();
        assert_eq!(
            record.publication.state,
            FeedbackPublicationState::RetryableFailed
        );
        assert_eq!(record.publication.next_attempt_at_ms, deadline);
        let searches = fixture.provider.searches.load(Ordering::Relaxed);
        let creates = fixture.provider.creates.load(Ordering::Relaxed);
        let reopened = FeedbackService::new(
            fixture.store.clone(),
            fixture.provider.clone(),
            "test-host".into(),
        );
        let deferred = reopened.submit(&record.id).await.unwrap();
        assert_eq!(deferred.publication.next_attempt_at_ms, deadline);
        assert_eq!(fixture.provider.searches.load(Ordering::Relaxed), searches);
        assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), creates);
    }
}
