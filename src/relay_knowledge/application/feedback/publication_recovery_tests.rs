use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::*;
use crate::application::feedback::test_support::*;

#[tokio::test]
async fn independent_outbox_deduplicates_public_content_without_publishing_private_fingerprints() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .configure(policy())
        .await
        .expect("first runtime policy");
    let first = fixture
        .service
        .report(report(), &context())
        .await
        .expect("first publication");
    let other = fixture.fresh_installation();
    other
        .configure(policy())
        .await
        .expect("independent authorization");
    fixture
        .provider
        .found
        .lock()
        .expect("responses")
        .push_back(Ok(Some(remote_issue("open"))));
    let duplicate = other
        .report(report(), &context())
        .await
        .expect("independent remote dedup");
    assert_ne!(
        duplicate.marker, first.marker,
        "nonce belongs to one durable outbox record"
    );
    assert_eq!(
        duplicate.publication.state,
        FeedbackPublicationState::Deduplicated
    );
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
    let first_payload = first.publication.payload.expect("first payload");
    let second_payload = duplicate.publication.payload.expect("second payload");
    assert_eq!(first_payload.dedup_marker, second_payload.dedup_marker);
    let markers = fixture.provider.markers.lock().expect("lookup markers");
    assert_eq!(
        markers.as_slice(),
        &[
            first_payload.dedup_marker.clone(),
            second_payload.dedup_marker
        ]
    );
    assert!(!first_payload.dedup_marker.contains(&first.fingerprint));
}

#[tokio::test]
async fn uncertain_send_uses_its_own_nonce_instead_of_shared_public_content_marker() {
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
        .push_back(Err(failure(FeedbackProviderErrorKind::Ambiguous)));
    let record = fixture
        .service
        .report(report(), &context())
        .await
        .expect("uncertain send preserved");
    fixture.clear_backoff(&record.id).await;
    fixture
        .service
        .submit(&record.id)
        .await
        .expect("read-only nonce recovery");
    let markers = fixture.provider.markers.lock().expect("lookup markers");
    assert_eq!(
        markers[0],
        record
            .publication
            .payload
            .as_ref()
            .expect("payload")
            .dedup_marker
    );
    assert_eq!(markers[1], record.marker);
    assert_ne!(markers[0], markers[1]);
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn report_recovers_authoritative_success_after_final_commit_acknowledgement_is_lost() {
    let fixture = Fixture::new().await;
    let commits = Arc::new(AtomicUsize::new(0));
    let service = FeedbackService::new(
        Arc::new(FaultAfterCommitStore {
            inner: fixture.store.clone(),
            commits: commits.clone(),
            fail_at: 4,
        }),
        fixture.provider.clone(),
        "test-host".into(),
    );
    service.configure(policy()).await.expect("commit1 policy");
    let record = service
        .report(report(), &context())
        .await
        .expect("successful final snapshot is recovered");
    assert_eq!(commits.load(Ordering::Relaxed), 4);
    assert_eq!(
        record.publication.state,
        FeedbackPublicationState::Submitted
    );
    assert_eq!(record.publication.issue, Some(remote_issue("open")));
    assert_eq!(record.publication.attempts, 1);
    assert_eq!(fixture.provider.creates.load(Ordering::Relaxed), 1);
    assert_eq!(
        service
            .status(Some(&record.id))
            .await
            .expect("durable status")["feedback"]["publication"]["state"],
        "submitted"
    );
}
