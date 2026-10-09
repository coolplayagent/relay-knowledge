use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::sync::{MutexGuard, Notify};

use super::*;
use crate::{
    api::InterfaceKind,
    env::{EnvironmentConfig, PlatformKind, RELAY_KNOWLEDGE_HOME},
    paths::RuntimePaths,
    ports::feedback::*,
    ports::feedback_store::{
        FeedbackJournal, FeedbackStoreError, FeedbackStoreErrorKind, FeedbackStoreFuture,
    },
    storage::feedback::FileFeedbackStore,
};

pub(super) struct Fixture {
    pub service: FeedbackService,
    pub store: Arc<FileFeedbackStore>,
    pub provider: Arc<MockProvider>,
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}

impl Fixture {
    pub async fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let guard = crate::storage::feedback::TEST_LOCK.lock().await;
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("canonical temp")
            .join(format!(
                "feedback-workflow-{}-{}-{}",
                std::process::id(),
                now_ms().expect("clock"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::current(),
            [(RELAY_KNOWLEDGE_HOME, root.clone())],
        )
        .expect("isolated environment");
        let paths =
            RuntimePaths::resolve(&environment.platform, &environment.paths).expect("test paths");
        let store = Arc::new(FileFeedbackStore::new(&paths));
        let provider = Arc::new(MockProvider::default());
        Self {
            service: FeedbackService::new(store.clone(), provider.clone(), "test-host".into()),
            store,
            provider,
            root,
            _guard: guard,
        }
    }

    pub async fn clear_backoff(&self, id: &str) {
        let mut transaction = self.store.begin().await.expect("open journal");
        let record = transaction
            .snapshot_mut()
            .records
            .iter_mut()
            .find(|record| record.id == id)
            .expect("stored report");
        record.publication.next_attempt_at_ms = 0;
        transaction.commit().await.expect("simulate retry deadline");
    }

    pub fn fresh_installation(&self) -> FeedbackService {
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::current(),
            [(RELAY_KNOWLEDGE_HOME, self.root.join("independent-runtime"))],
        )
        .expect("second runtime");
        let paths = RuntimePaths::resolve(&environment.platform, &environment.paths)
            .expect("independent paths");
        FeedbackService::new(
            Arc::new(FileFeedbackStore::new(&paths)),
            self.provider.clone(),
            "test-host".into(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Default)]
pub(super) struct MockProvider {
    pub creates: AtomicUsize,
    pub searches: AtomicUsize,
    pub markers: Mutex<Vec<String>>,
    pub reads: AtomicUsize,
    pub found: Mutex<VecDeque<Result<Option<FeedbackIssue>, FeedbackProviderError>>>,
    pub create_results: Mutex<VecDeque<Result<FeedbackIssue, FeedbackProviderError>>>,
    pub remote_state: Mutex<String>,
    pub payloads: Mutex<Vec<(String, String, String)>>,
    pub wait_on_create: std::sync::atomic::AtomicBool,
    pub create_started: Notify,
    pub release_create: Notify,
}

impl FeedbackProvider for MockProvider {
    fn find_marker<'a>(
        &'a self,
        target: &'a str,
        marker: &'a str,
    ) -> FeedbackProviderFuture<'a, Option<FeedbackIssue>> {
        Box::pin(async move {
            assert_eq!(target, "acme/project");
            assert!(marker.starts_with("<!-- relay-feedback:"));
            self.markers.lock().expect("markers").push(marker.into());
            self.searches.fetch_add(1, Ordering::Relaxed);
            self.found
                .lock()
                .expect("responses")
                .pop_front()
                .unwrap_or(Ok(None))
        })
    }

    fn create_issue<'a>(
        &'a self,
        target: &'a str,
        title: &'a str,
        body: &'a str,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue> {
        Box::pin(async move {
            self.creates.fetch_add(1, Ordering::Relaxed);
            self.payloads.lock().expect("payloads").push((
                target.into(),
                title.into(),
                body.into(),
            ));
            self.create_started.notify_one();
            if self.wait_on_create.load(Ordering::Relaxed) {
                self.release_create.notified().await;
            }
            self.create_results
                .lock()
                .expect("responses")
                .pop_front()
                .unwrap_or_else(|| Ok(remote_issue("open")))
        })
    }

    fn read_issue<'a>(
        &'a self,
        target: &'a str,
        number: u64,
    ) -> FeedbackProviderFuture<'a, FeedbackIssue> {
        Box::pin(async move {
            assert_eq!(target, "acme/project");
            assert_eq!(number, 12);
            self.reads.fetch_add(1, Ordering::Relaxed);
            Ok(remote_issue(&self.remote_state.lock().expect("state")))
        })
    }
}

pub(super) fn context() -> RequestContext {
    RequestContext::with_ids(
        InterfaceKind::Cli,
        "request-feedback-test",
        "trace-feedback-test",
    )
}

pub(super) fn report() -> FeedbackReport {
    FeedbackReport {
        schema_version: 1,
        kind: FeedbackKind::WorkflowFriction,
        intent: "Apply a research source batch".into(),
        expected: "One logical map publication".into(),
        actual: "Ten independent map publications".into(),
        impact: "Consumes recent history and requires a coordinating script".into(),
        observations: vec![],
        evidence: vec![],
        reproduction: Some(FeedbackReproduction {
            scenario: "Apply ten new source descriptions as one operation".into(),
            expected: "map version increases by one".into(),
        }),
        trace_id: None,
        request_id: None,
        diagnostics: Some(FeedbackDiagnostics {
            command: Some("map batch".into()),
            exit_status: Some(0),
            freshness: Some("fresh".into()),
            content_integrity: Some("verified".into()),
            environment: Some("fixed local fixture".into()),
            elapsed_ms: Some(900),
            steps: Some(10),
        }),
    }
}

pub(super) fn policy() -> FeedbackPolicy {
    FeedbackPolicy {
        mode: FeedbackMode::AutoSubmit,
        target_repository: Some("acme/project".into()),
        allowed_kinds: vec![FeedbackKind::WorkflowFriction],
        daily_quota: 5,
        ..Default::default()
    }
}

pub(super) fn remote_issue(state: &str) -> FeedbackIssue {
    FeedbackIssue {
        number: 12,
        url: "https://github.com/acme/project/issues/12".into(),
        state: state.into(),
        body_digest: feedback_digest(b"observed public feedback body"),
    }
}

pub(super) fn failure(kind: FeedbackProviderErrorKind) -> FeedbackProviderError {
    FeedbackProviderError {
        kind,
        message: "controlled provider failure".into(),
        retry_not_before_ms: None,
    }
}

pub(super) struct FaultAfterCommitStore {
    pub inner: Arc<FileFeedbackStore>,
    pub commits: Arc<AtomicUsize>,
    pub fail_at: usize,
}

impl FeedbackStore for FaultAfterCommitStore {
    fn begin(&self) -> FeedbackStoreFuture<'_, Box<dyn FeedbackTransaction>> {
        Box::pin(async move {
            Ok(Box::new(FaultAfterCommitTransaction {
                inner: self.inner.begin().await?,
                commits: self.commits.clone(),
                fail_at: self.fail_at,
            }) as Box<dyn FeedbackTransaction>)
        })
    }
}

struct FaultAfterCommitTransaction {
    inner: Box<dyn FeedbackTransaction>,
    commits: Arc<AtomicUsize>,
    fail_at: usize,
}

impl FeedbackTransaction for FaultAfterCommitTransaction {
    fn snapshot(&self) -> &FeedbackJournal {
        self.inner.snapshot()
    }
    fn snapshot_mut(&mut self) -> &mut FeedbackJournal {
        self.inner.snapshot_mut()
    }
    fn commit(&mut self) -> FeedbackStoreFuture<'_, ()> {
        Box::pin(async move {
            self.inner.commit().await?;
            if self.commits.fetch_add(1, Ordering::Relaxed) + 1 == self.fail_at {
                return Err(FeedbackStoreError {
                    kind: FeedbackStoreErrorKind::Io,
                    message: "response lost after durable commit".into(),
                });
            }
            Ok(())
        })
    }
}
