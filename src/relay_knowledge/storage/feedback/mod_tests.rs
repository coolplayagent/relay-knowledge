use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::{
    domain::feedback::*,
    env::{EnvironmentConfig, PlatformKind},
    paths::RuntimePaths,
    ports::feedback_store::{
        FeedbackStore, FeedbackStoreErrorKind, FeedbackTransaction, MAX_FEEDBACK_JOURNAL_BYTES,
        MAX_FEEDBACK_RECORDS,
    },
};

use super::{FileFeedbackStore, TEST_LOCK, validation};

struct Fixture {
    root: PathBuf,
    runtime: RuntimePaths,
    store: FileFeedbackStore,
}

impl Fixture {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "relay-feedback-store-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        let environment = EnvironmentConfig::from_pairs(
            PlatformKind::current(),
            [("RELAY_KNOWLEDGE_HOME", root.as_os_str())],
        )
        .unwrap();
        let runtime = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
        let store = FileFeedbackStore::new(&runtime);
        Self {
            root,
            runtime,
            store,
        }
    }

    async fn begin(&self) -> Box<dyn FeedbackTransaction> {
        // Other adapter tests share the process-wide four-worker budget.
        for _ in 0..100 {
            match self.store.begin().await {
                Ok(transaction) => return transaction,
                Err(error) if error.kind == FeedbackStoreErrorKind::Busy => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(error) => panic!("cannot open feedback fixture: {error}"),
            }
        }
        panic!("feedback fixture admission timed out")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn record() -> FeedbackRecord {
    let report: FeedbackReport = serde_json::from_value(serde_json::json!({
        "schema_version": 1, "kind": "workflow-friction", "intent": "Batch map updates",
        "expected": "One bounded request", "actual": "Multiple manual requests", "impact": "Extra workflow steps",
        "reproduction": {"scenario": "Update two map entries", "expected": "two entries updated"},
        "evidence": [{"label": "local-only", "content": "private raw evidence"}]
    })).unwrap();
    let marker = "<!-- relay-feedback:0123456789abcdef0123456789abcdef -->".to_owned();
    FeedbackRecord {
        raw_report_digest: feedback_digest(&serde_json::to_vec(&report).unwrap()),
        id: "0123456789abcdef0123456789abcdef".into(),
        fingerprint: report.fingerprint("1.1.18").unwrap(),
        publication: FeedbackPublication {
            state: FeedbackPublicationState::Draft,
            reason: None,
            target_repository: None,
            payload: Some(prepare_payload(&report, "1.1.18", "test-host", &marker).unwrap()),
            issue: None,
            attempts: 0,
            next_attempt_at_ms: 0,
        },
        marker,
        report,
        cli_version: "1.1.18".into(),
        platform: "test-host".into(),
        created_at_ms: 1,
        updated_at_ms: 1,
        occurrences: 1,
        validation: FeedbackValidationStatus {
            state: FeedbackVerificationState::AwaitingFix,
            fix: None,
            runs: Vec::new(),
        },
    }
}

#[tokio::test]
async fn commits_keep_the_exclusive_lock_and_reopen_preserves_evidence_and_quota() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new();
    assert!(!fixture.runtime.feedback_store_paths().directory.exists());
    let mut transaction = fixture.begin().await;
    transaction.snapshot_mut().records.push(record());
    transaction.snapshot_mut().quota_attempts = 1;
    transaction.snapshot_mut().quota_window_start_ms = 86_400_000;
    transaction.commit().await.unwrap();
    let error = fixture.store.begin().await.err().unwrap();
    assert_eq!(error.kind, FeedbackStoreErrorKind::Busy);
    transaction.snapshot_mut().records[0].occurrences += 1;
    transaction.commit().await.unwrap();
    drop(transaction);
    let reopened = fixture.begin().await;
    assert_eq!(reopened.snapshot().quota_attempts, 1);
    assert_eq!(reopened.snapshot().quota_window_start_ms, 86_400_000);
    assert_eq!(reopened.snapshot().records[0].occurrences, 2);
    assert_eq!(
        reopened.snapshot().records[0].report.evidence[0].content,
        "private raw evidence"
    );
}

#[tokio::test]
async fn uncommitted_changes_and_interrupted_prepared_file_never_replace_authority() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new();
    let mut transaction = fixture.begin().await;
    transaction.snapshot_mut().records.push(record());
    transaction.commit().await.unwrap();
    transaction.snapshot_mut().records[0].occurrences = 55;
    drop(transaction);
    let paths = fixture.runtime.feedback_store_paths();
    std::fs::write(
        &paths.prepared,
        b"partial journal after process interruption",
    )
    .unwrap();
    let reopened = fixture.begin().await;
    assert_eq!(reopened.snapshot().records[0].occurrences, 1);
    assert!(!paths.prepared.exists());
}

#[tokio::test]
async fn durable_send_intent_and_pinned_request_survive_interruption() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new();
    let mut transaction = fixture.begin().await;
    let mut entry = record();
    entry.publication.state = FeedbackPublicationState::Publishing;
    entry.publication.attempts = 1;
    entry.publication.target_repository = Some("example/project".into());
    transaction.snapshot_mut().records.push(entry);
    transaction.snapshot_mut().quota_attempts = 1;
    transaction.commit().await.unwrap();
    drop(transaction);
    let mut reopened = fixture.begin().await;
    assert_eq!(
        reopened.snapshot().records[0].publication.state,
        FeedbackPublicationState::Publishing
    );
    reopened.snapshot_mut().records[0]
        .publication
        .target_repository = Some("other/project".into());
    assert_eq!(
        reopened.commit().await.unwrap_err().kind,
        FeedbackStoreErrorKind::InvalidData
    );
    assert!(
        reopened
            .commit()
            .await
            .unwrap_err()
            .message
            .contains("interrupted")
    );
    drop(reopened);
    assert_eq!(
        fixture.begin().await.snapshot().records[0]
            .publication
            .target_repository
            .as_deref(),
        Some("example/project")
    );
}

#[tokio::test]
async fn malformed_or_oversized_journal_is_preserved_and_never_reset() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new();
    drop(fixture.begin().await);
    let paths = fixture.runtime.feedback_store_paths();
    std::fs::write(&paths.journal, b"not valid json").unwrap();
    assert_eq!(
        fixture.store.begin().await.err().unwrap().kind,
        FeedbackStoreErrorKind::InvalidData
    );
    assert_eq!(std::fs::read(&paths.journal).unwrap(), b"not valid json");
    let file = std::fs::File::create(&paths.journal).unwrap();
    file.set_len(MAX_FEEDBACK_JOURNAL_BYTES as u64 + 1).unwrap();
    drop(file);
    assert_eq!(
        fixture.store.begin().await.err().unwrap().kind,
        FeedbackStoreErrorKind::Capacity
    );
}

#[tokio::test]
async fn failed_atomic_publication_preserves_old_journal_and_requires_reopen() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new();
    let mut transaction = fixture.begin().await;
    transaction.snapshot_mut().records.push(record());
    transaction.commit().await.unwrap();
    let paths = fixture.runtime.feedback_store_paths();
    std::fs::create_dir(&paths.prepared).unwrap();
    transaction.snapshot_mut().records[0].occurrences += 1;
    assert_eq!(
        transaction.commit().await.unwrap_err().kind,
        FeedbackStoreErrorKind::Io
    );
    drop(transaction);
    assert_eq!(
        fixture.store.begin().await.err().unwrap().kind,
        FeedbackStoreErrorKind::Io
    );
    std::fs::remove_dir(&paths.prepared).unwrap();
    assert_eq!(fixture.begin().await.snapshot().records[0].occurrences, 1);
}

#[cfg(unix)]
#[tokio::test]
async fn files_are_private_and_symlinks_and_hardlinks_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new();
    let mut transaction = fixture.begin().await;
    transaction.commit().await.unwrap();
    drop(transaction);
    let paths = fixture.runtime.feedback_store_paths();
    assert_eq!(
        std::fs::metadata(&paths.directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for path in [&paths.lock, &paths.journal] {
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::remove_file(&paths.journal).unwrap();
    symlink(&paths.lock, &paths.journal).unwrap();
    assert_eq!(
        fixture.store.begin().await.err().unwrap().kind,
        FeedbackStoreErrorKind::Io
    );
    std::fs::remove_file(&paths.journal).unwrap();
    std::fs::hard_link(&paths.lock, &paths.journal).unwrap();
    assert_eq!(
        fixture.store.begin().await.err().unwrap().kind,
        FeedbackStoreErrorKind::Io
    );
    std::fs::remove_file(&paths.journal).unwrap();
    std::fs::remove_file(&paths.lock).unwrap();
    std::fs::remove_dir(&paths.directory).unwrap();
    symlink(&fixture.root, &paths.directory).unwrap();
    assert_eq!(
        fixture.store.begin().await.err().unwrap().kind,
        FeedbackStoreErrorKind::Io
    );
}

#[test]
fn journal_rejects_record_capacity_corruption_and_loss_of_evidence() {
    let mut old = crate::ports::feedback_store::FeedbackJournal::default();
    old.records.push(record());
    validation::validate_snapshot(&old).unwrap();
    let mut next = old.clone();
    next.schema_version = 2;
    assert!(validation::validate_snapshot(&next).is_err());
    next = old.clone();
    next.records = vec![record(); MAX_FEEDBACK_RECORDS + 1];
    assert_eq!(
        validation::validate_snapshot(&next).unwrap_err().kind,
        FeedbackStoreErrorKind::Capacity
    );
    next = old.clone();
    next.records.push(record());
    assert!(validation::validate_snapshot(&next).is_err());
    next = old.clone();
    next.records.clear();
    assert!(validation::validate_transition(&old, &next).is_err());
    next = old.clone();
    next.records[0].report.actual = "modified evidence".into();
    assert!(validation::validate_snapshot(&next).is_err());
    next.records[0].fingerprint = next.records[0].report.fingerprint("1.1.18").unwrap();
    assert!(validation::validate_transition(&old, &next).is_err());
    next = old.clone();
    next.records[0].occurrences = 0;
    assert!(validation::validate_snapshot(&next).is_err());
    next = old.clone();
    next.records[0].publication.attempts = 1;
    assert!(validation::validate_snapshot(&next).is_err());
    next = old.clone();
    next.records[0].publication.state = FeedbackPublicationState::Submitted;
    assert!(validation::validate_snapshot(&next).is_err());
    old.quota_attempts = 2;
    next = old.clone();
    next.quota_attempts = 1;
    assert!(validation::validate_transition(&old, &next).is_err());
}

#[test]
fn content_bindings_detect_private_evidence_and_public_payload_tampering() {
    let mut journal = crate::ports::feedback_store::FeedbackJournal::default();
    journal.records.push(record());
    let mut changed = journal.clone();
    changed.records[0].report.evidence[0].content = "replaced private evidence".into();
    assert_eq!(
        changed.records[0].report.fingerprint("1.1.18").unwrap(),
        journal.records[0].fingerprint
    );
    assert!(
        validation::validate_snapshot(&changed)
            .unwrap_err()
            .message
            .contains("raw evidence")
    );
    changed = journal.clone();
    changed.records[0]
        .publication
        .payload
        .as_mut()
        .unwrap()
        .body
        .push_str(" injected content");
    assert!(
        validation::validate_snapshot(&changed)
            .unwrap_err()
            .message
            .contains("public payload")
    );
    changed = journal.clone();
    changed.records[0]
        .publication
        .payload
        .as_mut()
        .unwrap()
        .digest
        .clear();
    assert!(validation::validate_snapshot(&changed).is_err());
    changed = journal.clone();
    changed.records[0].validation.state = FeedbackVerificationState::VerifiedFixed;
    assert!(validation::validate_snapshot(&changed).is_err());
}

#[test]
fn recovery_cannot_erase_uncertainty_or_bypass_send_quota() {
    let mut prior = crate::ports::feedback_store::FeedbackJournal::default();
    prior.records.push(record());
    let mut next = prior.clone();
    next.records[0].publication.state = FeedbackPublicationState::Publishing;
    next.records[0].publication.attempts = 1;
    next.records[0].publication.target_repository = Some("example/project".into());
    assert!(
        validation::validate_transition(&prior, &next)
            .unwrap_err()
            .message
            .contains("quota accounting")
    );
    next.quota_attempts = 1;
    validation::validate_transition(&prior, &next).unwrap();
    prior = next.clone();
    next.records[0].publication.state = FeedbackPublicationState::RetryableFailed;
    // A definitive rejection during the original transaction is recoverable.
    validation::validate_transition(&prior, &next).unwrap();
    // After reopening, the same transition would permit a duplicate POST.
    assert!(validation::validate_recovery(&prior, &next).is_err());
    next.records[0].publication.state = FeedbackPublicationState::AwaitingReconciliation;
    validation::validate_recovery(&prior, &next).unwrap();
    prior = next.clone();
    next.records[0].publication.state = FeedbackPublicationState::Publishing;
    next.records[0].publication.attempts += 1;
    next.quota_attempts += 1;
    assert!(validation::validate_transition(&prior, &next).is_err());
    next = prior.clone();
    next.quota_window_start_ms = 1;
    next.quota_attempts = 0;
    assert!(validation::validate_transition(&prior, &next).is_err());
    next.quota_window_start_ms = 86_400_000;
    validation::validate_transition(&prior, &next).unwrap();
}

#[tokio::test]
async fn terminated_writer_releases_os_lock_and_retains_pre_send_claim() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new();
    let ready = fixture.root.join("child-ready");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "storage::feedback::tests::holds_lock_in_child_process",
            "--nocapture",
        ])
        .env("RELAY_FEEDBACK_CHILD_FIXTURE", &fixture.root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = ChildGuard(child);
    for _ in 0..300 {
        if ready.exists() {
            break;
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "writer exited before its durable claim"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        ready.exists(),
        "writer did not persist its claim within the test budget"
    );
    assert_eq!(
        fixture.store.begin().await.err().unwrap().kind,
        FeedbackStoreErrorKind::Busy
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let reopened = fixture.begin().await;
    assert_eq!(
        reopened.snapshot().records[0].publication.state,
        FeedbackPublicationState::Publishing
    );
    assert_eq!(reopened.snapshot().quota_attempts, 1);
}

#[tokio::test]
async fn holds_lock_in_child_process() {
    // Only the crash-recovery test opts into this bounded subprocess fixture.
    let Some(root) = std::env::var_os("RELAY_FEEDBACK_CHILD_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [("RELAY_KNOWLEDGE_HOME", root.as_os_str())],
    )
    .unwrap();
    let runtime = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let store = FileFeedbackStore::new(&runtime);
    let mut transaction = store.begin().await.unwrap();
    let mut entry = record();
    entry.publication.state = FeedbackPublicationState::Publishing;
    entry.publication.attempts = 1;
    entry.publication.target_repository = Some("example/project".into());
    transaction.snapshot_mut().records.push(entry);
    transaction.snapshot_mut().quota_attempts = 1;
    transaction.commit().await.unwrap();
    std::fs::write(root.join("child-ready"), b"ready").unwrap();
    tokio::time::sleep(Duration::from_secs(30)).await;
}
