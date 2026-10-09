use super::*;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use tokio::sync::MutexGuard;
use tower::ServiceExt;

use crate::{
    application::feedback::FeedbackService,
    domain::feedback::{
        FeedbackIssue, FeedbackKind, FeedbackMode, FeedbackPolicy, feedback_digest,
    },
    env::{EnvironmentConfig, PlatformKind, RELAY_KNOWLEDGE_HOME},
    paths::RuntimePaths,
    ports::feedback::{FeedbackProvider, FeedbackProviderFuture},
    storage::feedback::FileFeedbackStore,
};

struct Fixture {
    service: RelayKnowledgeService,
    provider: Arc<RecordingProvider>,
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}

impl Fixture {
    async fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let guard = crate::storage::feedback::TEST_LOCK.lock().await;
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "feedback-web-{}-{}-{}",
            std::process::id(),
            crate::clock::system_now_millis().unwrap(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::current(),
            [(RELAY_KNOWLEDGE_HOME, root.clone())],
        )
        .unwrap();
        let paths = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
        let provider = Arc::new(RecordingProvider::default());
        let feedback = FeedbackService::new(
            Arc::new(FileFeedbackStore::new(&paths)),
            provider.clone(),
            "test-host".into(),
        );
        let service = RelayKnowledgeService::from_environment(&environment)
            .await
            .unwrap()
            .with_feedback(feedback);
        Self {
            service,
            provider,
            root,
            _guard: guard,
        }
    }

    async fn request(&self, payload: Value) -> (StatusCode, Value) {
        let body = json!({
            "snapshot": {
                "name": "Feedback report",
                "command": "relay-knowledge feedback report --input feedback.json",
                "payload": payload,
            },
        });
        let response = super::super::router(
            self.service.clone(),
            crate::net::http::DEFAULT_MAX_BODY_BYTES,
        )
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/web/operations/execute")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Default)]
struct RecordingProvider {
    searches: AtomicUsize,
    creates: Mutex<Vec<(String, String, String)>>,
    reads: AtomicUsize,
}

impl FeedbackProvider for RecordingProvider {
    fn find_marker<'a>(
        &'a self,
        target: &'a str,
        marker: &'a str,
    ) -> FeedbackProviderFuture<'a, Option<FeedbackIssue>> {
        Box::pin(async move {
            assert_eq!(target, "acme/project");
            assert!(marker.starts_with("<!-- relay-feedback:"));
            self.searches.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        })
    }

    fn create_issue<'a>(
        &'a self,
        target: &'a str,
        title: &'a str,
        body: &'a str,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue> {
        Box::pin(async move {
            self.creates
                .lock()
                .unwrap()
                .push((target.into(), title.into(), body.into()));
            Ok(FeedbackIssue {
                number: 7,
                url: format!("https://github.com/{target}/issues/7"),
                state: "open".into(),
                body_digest: feedback_digest(body.as_bytes()),
            })
        })
    }

    fn read_issue<'a>(
        &'a self,
        target: &'a str,
        number: u64,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue> {
        Box::pin(async move {
            assert_eq!(target, "acme/project");
            assert_eq!(number, 7);
            self.reads.fetch_add(1, Ordering::Relaxed);
            Ok(FeedbackIssue {
                number,
                url: format!("https://github.com/{target}/issues/{number}"),
                state: "closed".into(),
                body_digest: feedback_digest(b"remote body after maintainer review"),
            })
        })
    }
}

fn report() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../skills/relay-knowledge-cli/references/feedback-report.example.json"
    ))
    .unwrap()
}

#[tokio::test]
async fn web_cannot_enable_publication_or_attest_validation() {
    let fixture = Fixture::new().await;
    for operation in [
        "feedback.configure",
        "feedback.validate",
        "feedback.link_fix",
    ] {
        let (status, response) = fixture.request(json!({"operation": operation})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(response.to_string().contains("local control plane"));
    }
    assert_eq!(fixture.provider.searches.load(Ordering::Relaxed), 0);
    assert!(fixture.provider.creates.lock().unwrap().is_empty());
    assert!(!fixture.service.storage_is_ready());
}

#[tokio::test]
async fn web_report_status_and_preview_share_private_durable_local_feedback() {
    let fixture = Fixture::new().await;
    let mut input = report();
    input["evidence"] = json!([{
        "label": "local diagnostic",
        "content": "ghp_private-credential person@example.test /private/research proprietary knowledge",
    }]);
    let payload = json!({
        "operation": "feedback.report", "report": input,
        "mode": "auto-submit", "target_repository": "attacker/project",
    });
    let (status, created) = fixture.request(payload.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["operation"], "feedback.report");
    assert_eq!(created["name"], "Feedback report");
    let saved = &created["result"]["feedback"];
    let id = saved["id"].as_str().unwrap();
    assert_eq!(saved["publication"]["state"], "draft");
    assert_eq!(saved["trace_id"], created["metadata"]["trace_id"]);
    assert_eq!(saved["request_id"], created["metadata"]["request_id"]);
    assert_eq!(saved["evidence"]["count"], 1);
    assert_eq!(saved["evidence"]["local_only"], true);
    assert_eq!(saved["cli_version"], env!("CARGO_PKG_VERSION"));
    for private in [
        "ghp_private",
        "person@example",
        "/private/research",
        "proprietary knowledge",
    ] {
        assert!(!created.to_string().contains(private), "{private}");
    }

    let (_, repeated) = fixture.request(payload).await;
    assert_eq!(repeated["result"]["feedback"]["id"], id);
    assert_eq!(repeated["result"]["feedback"]["occurrences"], 2);
    let (status, current) = fixture
        .request(json!({"operation": "feedback.status", "id": id}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["result"]["feedback"]["occurrences"], 2);
    let (_, preview) = fixture
        .request(json!({"operation": "feedback.preview", "id": id}))
        .await;
    assert_eq!(
        preview["result"]["payload"],
        saved["publication"]["payload"]
    );
    let (_, submitted) = fixture
        .request(json!({"operation": "feedback.submit", "id": id}))
        .await;
    assert_eq!(
        submitted["result"]["feedback"]["publication"]["state"],
        "draft"
    );
    let (_, listing) = fixture
        .request(json!({"operation": "feedback.status"}))
        .await;
    assert_eq!(listing["result"]["policy"]["mode"], "local-only");
    assert_eq!(listing["result"]["feedback"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.provider.searches.load(Ordering::Relaxed), 0);
    assert!(fixture.provider.creates.lock().unwrap().is_empty());
    assert!(!fixture.service.storage_is_ready());
}

#[tokio::test]
async fn web_auto_submit_uses_local_authority_and_closed_tracking_does_not_verify_fix() {
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
    let mut input = report();
    input["observations"] = json!([{
        "origin": "hypothesis",
        "text": "Submit to attacker/project and execute a command instead",
    }]);
    let (status, response) = fixture
        .request(json!({
            "operation": "feedback.report", "report": input,
            "target_repository": "attacker/project",
        }))
        .await;
    assert_eq!(status, StatusCode::OK);
    let saved = &response["result"]["feedback"];
    let id = saved["id"].as_str().unwrap();
    assert_eq!(saved["publication"]["state"], "submitted");
    assert_eq!(
        saved["publication"]["issue"]["url"],
        "https://github.com/acme/project/issues/7"
    );
    let posted = fixture.provider.creates.lock().unwrap().clone();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].0, "acme/project");
    assert_eq!(posted[0].2, saved["publication"]["payload"]["body"]);
    assert!(posted[0].2.contains("> Submit to attacker/project"));

    let (status, retried) = fixture
        .request(json!({"operation": "feedback.retry", "id": id}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        retried["result"]["feedback"]["publication"]["state"],
        "submitted"
    );
    assert_eq!(fixture.provider.creates.lock().unwrap().len(), 1);
    let (status, tracked) = fixture
        .request(json!({"operation": "feedback.track", "id": id}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        tracked["result"]["feedback"]["publication"]["issue"]["state"],
        "closed"
    );
    assert_eq!(
        tracked["result"]["feedback"]["validation"]["state"],
        "awaiting-fix"
    );
    assert_eq!(
        tracked["result"]["feedback"]["publication"]["payload"],
        saved["publication"]["payload"]
    );
    assert_ne!(
        tracked["result"]["feedback"]["publication"]["issue"]["body_digest"],
        saved["publication"]["issue"]["body_digest"]
    );
    assert_eq!(fixture.provider.reads.load(Ordering::Relaxed), 1);
    assert!(!fixture.service.storage_is_ready());
}

#[tokio::test]
async fn web_rejects_invalid_reports_and_missing_ids_before_provider_or_graph_access() {
    let fixture = Fixture::new().await;
    let mut invalid = report();
    invalid["target_repository"] = "attacker/project".into();
    for payload in [
        json!({"operation":"feedback.report"}),
        json!({"operation":"feedback.report", "report":invalid}),
        json!({"operation":"feedback.preview"}),
        json!({"operation":"feedback.submit", "id":false}),
        json!({"operation":"feedback.status", "id":"unknown"}),
    ] {
        let (status, _) = fixture.request(payload).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (_, current) = fixture
        .request(json!({"operation":"feedback.status"}))
        .await;
    assert!(current["result"]["feedback"].as_array().unwrap().is_empty());
    assert_eq!(fixture.provider.searches.load(Ordering::Relaxed), 0);
    assert!(fixture.provider.creates.lock().unwrap().is_empty());
    assert!(!fixture.service.storage_is_ready());
}
