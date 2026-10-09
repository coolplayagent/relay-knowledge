use super::*;
use crate::ports::{
    feedback::FeedbackProviderErrorKind,
    feedback_store::{
        FeedbackStore, FeedbackStoreError, FeedbackStoreErrorKind, FeedbackStoreFuture,
        FeedbackTransaction,
    },
};

struct UnavailableStore(FeedbackStoreErrorKind);

impl FeedbackStore for UnavailableStore {
    fn begin(&self) -> FeedbackStoreFuture<'_, Box<dyn FeedbackTransaction>> {
        Box::pin(async {
            Err(FeedbackStoreError {
                kind: self.0,
                message: "feedback storage unavailable".into(),
            })
        })
    }
}

#[tokio::test]
async fn web_storage_contention_returns_429_and_unlock_restores_requests() {
    let fixture = Fixture::new().await;
    let transaction = fixture.store.begin().await.unwrap();
    let (status, response) = fixture
        .request(json!({"operation": "feedback.status"}))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(response["error"].as_str().unwrap().contains("retry"));
    drop(transaction);
    let (status, _) = fixture
        .request(json!({"operation": "feedback.status"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!fixture.service.storage_is_ready());
}

#[tokio::test]
async fn web_storage_faults_return_503_without_hiding_invalid_report_errors() {
    let mut fixture = Fixture::new().await;
    for kind in [
        FeedbackStoreErrorKind::Io,
        FeedbackStoreErrorKind::InvalidData,
        FeedbackStoreErrorKind::Capacity,
    ] {
        fixture.service = fixture.service.clone().with_feedback(FeedbackService::new(
            Arc::new(UnavailableStore(kind)),
            fixture.provider.clone(),
            "test-host".into(),
        ));
        for payload in [
            json!({"operation": "feedback.status"}),
            json!({"operation": "feedback.report", "report": report()}),
        ] {
            let (status, response) = fixture.request(payload).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(response["error"], "feedback storage unavailable");
        }
        let mut invalid = report();
        invalid["intent"] = "".into();
        let (status, _) = fixture
            .request(json!({"operation": "feedback.report", "report": invalid}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    assert!(fixture.provider.creates.lock().unwrap().is_empty());
}

#[tokio::test]
async fn web_tracking_timeout_and_provider_unavailability_return_503() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .feedback_service()
        .unwrap()
        .configure(FeedbackPolicy {
            mode: FeedbackMode::AutoSubmit,
            target_repository: Some("acme/project".into()),
            allowed_kinds: vec![FeedbackKind::WorkflowFriction],
            ..Default::default()
        })
        .await
        .unwrap();
    let (status, created) = fixture
        .request(json!({"operation": "feedback.report", "report": report()}))
        .await;
    assert_eq!(status, StatusCode::OK);
    let id = created["result"]["feedback"]["id"].as_str().unwrap();
    for (kind, message) in [
        (
            FeedbackProviderErrorKind::Retryable,
            "GitHub request timed out",
        ),
        (FeedbackProviderErrorKind::Retryable, "GitHub unavailable"),
        (
            FeedbackProviderErrorKind::Rejected,
            "GitHub credentials are unavailable",
        ),
    ] {
        *fixture.provider.read_error.lock().unwrap() = Some(FeedbackProviderError {
            kind,
            message: message.into(),
            retry_not_before_ms: None,
        });
        let (status, response) = fixture
            .request(json!({"operation": "feedback.track", "id": id}))
            .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response["error"], message);
        let (status, current) = fixture
            .request(json!({"operation": "feedback.status", "id": id}))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            current["result"]["feedback"]["publication"],
            created["result"]["feedback"]["publication"]
        );
    }
    assert_eq!(fixture.provider.creates.lock().unwrap().len(), 1);
    assert_eq!(fixture.provider.reads.load(Ordering::Relaxed), 3);
}
