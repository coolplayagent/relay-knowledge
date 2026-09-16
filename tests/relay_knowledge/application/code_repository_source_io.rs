//! Real Windows sharing failures across registration, durable indexing, queries and repair.
#![cfg(windows)]

use relay_knowledge::{
    api::{CodeRepositoryRegisterRequest, InterfaceKind, RequestContext},
    application::{RelayKnowledgeService, RuntimeConfiguration},
    domain::{
        CodeContentIntegrityState, CodeDiagnosticsRequest, CodeIndexMode, CodeIndexRequest,
        CodeQueryKind, CodeRepositorySelector, CodeRetrievalRequest, FreshnessPolicy,
    },
    env::{EnvironmentConfig, PlatformKind},
    storage::{CodeIndexTaskStore as _, SqliteGraphStore},
};
use std::{
    fs,
    os::windows::fs::OpenOptionsExt,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new(git: bool) -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("relay-source-io-integration-{id}"));
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(
            path.join("src/A.java"),
            "class A { int originalValue() { return 1; } }",
        )
        .unwrap();
        fs::write(
            path.join("src/B.java"),
            "class B { int healthyValue() { return 2; } }",
        )
        .unwrap();
        let fixture = Self(path);
        if git {
            for args in [
                vec!["init"],
                vec!["config", "user.email", "io@example.invalid"],
                vec!["config", "user.name", "IO Test"],
                vec!["add", "."],
                vec!["commit", "-m", "base"],
            ] {
                assert!(
                    Command::new("git")
                        .current_dir(&fixture.0)
                        .args(args)
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
        }
        fixture
    }
    fn lock(&self) -> fs::File {
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(self.0.join("src/A.java"))
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn context() -> RequestContext {
    RequestContext::with_ids(InterfaceKind::Cli, "source-io", "source-io")
}
fn selector(reference: &str) -> CodeRepositorySelector {
    CodeRepositorySelector::new("fixture", reference, vec![], vec![]).unwrap()
}
fn request(reference: &str, mode: CodeIndexMode) -> CodeIndexRequest {
    CodeIndexRequest {
        repository: selector(if mode == CodeIndexMode::WorktreeOverlay {
            "HEAD"
        } else {
            reference
        }),
        mode,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    }
}
async fn service(fixture: &Fixture) -> (RelayKnowledgeService, Arc<SqliteGraphStore>) {
    let store = Arc::new(SqliteGraphStore::open_in_memory().unwrap());
    let runtime_root = fixture.0.join("runtime").to_string_lossy().into_owned();
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Windows,
        [
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "RELAY_KNOWLEDGE_HOME",
        ]
        .map(|key| (key, runtime_root.as_str())),
    )
    .unwrap();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .unwrap();
    let service = RelayKnowledgeService::with_store(runtime, store.clone());
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: fixture.0.to_string_lossy().into_owned(),
                alias: "fixture".into(),
                path_filters: vec!["src".into()],
                language_filters: vec![],
            },
            context(),
        )
        .await
        .unwrap();
    (service, store)
}

async fn verify_partial_and_repair(git: bool) {
    let fixture = Fixture::new(git);
    let (service, store) = service(&fixture).await;
    let base = service
        .index_code_repository(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap();
    fs::write(
        fixture.0.join("src/A.java"),
        "class A { int repairedValue() { return 3; } }",
    )
    .unwrap();
    fs::write(
        fixture.0.join("src/B.java"),
        "class B { int updatedHealthyValue() { return 4; } }",
    )
    .unwrap();
    let lock = fixture.lock();
    let partial = service
        .index_code_repository(
            request(
                "HEAD",
                if git {
                    CodeIndexMode::WorktreeOverlay
                } else {
                    CodeIndexMode::Incremental {
                        base_ref: base.summary.resolved_commit_sha.clone(),
                        head_ref: "HEAD".into(),
                    }
                },
            ),
            context(),
        )
        .await
        .unwrap();
    assert_eq!(partial.summary.indexed_file_count, 1);
    assert_eq!(partial.summary.progress.io_skipped_file_count, 1);
    let diagnostics = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: selector(&partial.summary.resolved_commit_sha),
                limit: 1,
                cursor: None,
            },
            context(),
        )
        .await
        .unwrap();
    assert_eq!(diagnostics.degraded_file_count, 1);
    assert_eq!(diagnostics.diagnostics[0].path, "src/A.java");
    assert_eq!(
        diagnostics.diagnostics[0].io.as_ref().unwrap().raw_os_error,
        Some(32)
    );
    assert_eq!(
        diagnostics.content_integrity.state,
        CodeContentIntegrityState::Partial
    );
    for kind in [CodeQueryKind::Definition, CodeQueryKind::Hybrid] {
        for (commit, expected) in [
            (&partial.summary.resolved_commit_sha, false),
            (&base.summary.resolved_commit_sha, true),
        ] {
            let query = service
                .query_code_repository(
                    CodeRetrievalRequest::new(
                        "originalValue",
                        selector(commit),
                        kind,
                        10,
                        FreshnessPolicy::AllowStale,
                    )
                    .unwrap(),
                    context(),
                )
                .await
                .unwrap();
            assert_eq!(
                query.results.iter().any(|hit| hit.path == "src/A.java"),
                expected
            );
        }
    }
    assert_ne!(partial.summary.source_scope, base.summary.source_scope);
    assert_eq!(
        store
            .code_index_task_queue_status()
            .await
            .unwrap()
            .dead_letter_task_count,
        0
    );
    drop(lock);
    let repaired = service
        .index_code_repository(
            request(
                "HEAD",
                if git {
                    CodeIndexMode::WorktreeOverlay
                } else {
                    CodeIndexMode::Incremental {
                        base_ref: partial.summary.resolved_commit_sha.clone(),
                        head_ref: "HEAD".into(),
                    }
                },
            ),
            context(),
        )
        .await
        .unwrap();
    assert_eq!(repaired.summary.indexed_file_count, 2);
    assert_eq!(repaired.summary.progress.io_skipped_file_count, 0);
    assert_ne!(repaired.summary.source_scope, partial.summary.source_scope);
    let diagnostics = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: selector(&repaired.summary.resolved_commit_sha),
                limit: 10,
                cursor: None,
            },
            context(),
        )
        .await
        .unwrap();
    assert!(diagnostics.diagnostics.is_empty());
    assert_eq!(
        diagnostics.content_integrity.state,
        CodeContentIntegrityState::Complete
    );
    assert_eq!(
        store
            .code_index_task_queue_status()
            .await
            .unwrap()
            .dead_letter_task_count,
        0
    );
}

#[tokio::test]
async fn git_worktree_skips_a_locked_java_file_and_repairs_without_reset() {
    verify_partial_and_repair(true).await;
}

#[tokio::test]
async fn filesystem_delta_skips_a_locked_java_file_and_repairs_without_reset() {
    verify_partial_and_repair(false).await;
}

#[tokio::test]
async fn filesystem_cold_index_can_complete_with_only_io_diagnostics() {
    let fixture = Fixture::new(false);
    fs::remove_file(fixture.0.join("src/B.java")).unwrap();
    let lock = fixture.lock();
    let (service, store) = service(&fixture).await;
    let result = service
        .index_code_repository(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap();
    assert_eq!(result.summary.indexed_file_count, 0);
    assert_eq!(result.summary.progress.parsed_file_count, 0);
    assert_eq!(result.summary.progress.io_skipped_file_count, 1);
    assert_eq!(
        store
            .code_index_task_queue_status()
            .await
            .unwrap()
            .dead_letter_task_count,
        0
    );
    let repeated = service
        .index_code_repository(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap();
    assert_eq!(repeated.summary.source_scope, result.summary.source_scope);
    assert_eq!(repeated.summary.progress.io_skipped_file_count, 1);
    drop(lock);
    let repaired = service
        .index_code_repository(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap();
    assert_eq!(repaired.summary.indexed_file_count, 1);
}

#[tokio::test]
async fn filesystem_directory_acl_skip_and_repair_clear_current_diagnostics() {
    let fixture = Fixture::new(false);
    let (service, _) = service(&fixture).await;
    let base = service
        .index_code_repository(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap();
    let output = Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let sid = whoami_user_sid(&output.stdout)
        .expect("whoami should return an ASCII SID independently of the username encoding")
        .to_owned();
    struct Deny {
        path: PathBuf,
        sid: String,
    }
    impl Drop for Deny {
        fn drop(&mut self) {
            let output = Command::new("icacls")
                .arg(&self.path)
                .args(["/remove:d", &format!("*{}", self.sid)])
                .output()
                .unwrap();
            assert!(output.status.success());
        }
    }
    let deny = Deny {
        path: fixture.0.join("src"),
        sid,
    };
    let output = Command::new("icacls")
        .arg(&deny.path)
        .args(["/deny", &format!("*{}:(RD)", deny.sid)])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_dir(&deny.path).unwrap_err().raw_os_error(),
        Some(5)
    );
    let partial = service
        .index_code_repository(
            request(
                "HEAD",
                CodeIndexMode::Incremental {
                    base_ref: base.summary.resolved_commit_sha.clone(),
                    head_ref: "HEAD".into(),
                },
            ),
            context(),
        )
        .await
        .unwrap();
    assert_eq!(partial.summary.indexed_file_count, 0);
    assert_eq!(partial.summary.progress.io_skipped_directory_count, 1);
    assert_eq!(partial.summary.progress.io_skipped_file_count, 0);
    let child_diagnostics = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: CodeRepositorySelector::new(
                    "fixture",
                    &partial.summary.resolved_commit_sha,
                    vec!["src/A.java".into()],
                    vec![],
                )
                .unwrap(),
                limit: 1,
                cursor: None,
            },
            context(),
        )
        .await
        .unwrap();
    assert_eq!(child_diagnostics.degraded_file_count, 0);
    assert_eq!(
        child_diagnostics
            .content_integrity
            .io_skipped_directory_count,
        Some(1)
    );
    assert_eq!(child_diagnostics.diagnostics.len(), 1);
    assert_eq!(child_diagnostics.diagnostics[0].path, "src");
    assert_eq!(
        child_diagnostics.diagnostics[0]
            .io
            .as_ref()
            .unwrap()
            .raw_os_error,
        Some(5)
    );
    assert!(child_diagnostics.next_cursor.is_none());
    drop(deny);
    let repaired = service
        .index_code_repository(
            request(
                "HEAD",
                CodeIndexMode::Incremental {
                    base_ref: partial.summary.resolved_commit_sha.clone(),
                    head_ref: "HEAD".into(),
                },
            ),
            context(),
        )
        .await
        .unwrap();
    assert_eq!(repaired.summary.indexed_file_count, 2);
    let diagnostics = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: selector(&repaired.summary.resolved_commit_sha),
                limit: 10,
                cursor: None,
            },
            context(),
        )
        .await
        .unwrap();
    assert_eq!(
        diagnostics.content_integrity.state,
        CodeContentIntegrityState::Complete
    );
    assert!(diagnostics.diagnostics.is_empty());
}

fn whoami_user_sid(output: &[u8]) -> Option<&str> {
    // whoami encodes the localized account name in the Windows console code page.
    // Only the final CSV field is a SID; it is ASCII in every locale.
    let mut fields = output.rsplitn(2, |byte| *byte == b',');
    let sid = std::str::from_utf8(fields.next()?)
        .ok()?
        .trim()
        .trim_matches('"');
    fields.next()?;
    sid.strip_prefix("S-1-")?
        .split('-')
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(sid)
}

#[test]
fn source_io_whoami_sid_ignores_localized_username_encoding_and_rejects_invalid_sids() {
    assert_eq!(
        whoami_user_sid(b"\"HOST\\\xd5\xc5\xc8\xfd\",\"S-1-5-21-123-456-1001\"\r\n"),
        Some("S-1-5-21-123-456-1001")
    );
    assert_eq!(
        whoami_user_sid(b"\"HOST\\account\",\"S-1-5-18\"\r\n"),
        Some("S-1-5-18")
    );
    for invalid in [
        b"S-1-5-18".as_slice(),
        b"\"account\",\"S-1-\"",
        b"\"account\",\"S-1-5-x\"",
        b"\"account\",\"S-1-5-\xff\"",
    ] {
        assert_eq!(whoami_user_sid(invalid), None);
    }
}

#[tokio::test]
async fn filesystem_failure_after_queue_rebinds_the_same_attempt_and_keeps_lease_authority() {
    let fixture = Fixture::new(false);
    let (service, store) = service(&fixture).await;
    let queued = service
        .start_code_repository_index(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap()
        .task
        .unwrap();
    let lock = fixture.lock();
    let completed = service
        .run_code_index_task_once(Some(queued.task_id.clone()), context())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        completed.state,
        relay_knowledge::domain::CodeIndexTaskState::Succeeded
    );
    assert_eq!(completed.attempt_count, 1);
    assert_eq!(completed.task_id, queued.task_id);
    assert_ne!(completed.source_scope, queued.source_scope);
    assert!(completed.last_error_message.is_none());
    let diagnostics = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: selector(&completed.resolved_commit_sha),
                limit: 10,
                cursor: None,
            },
            context(),
        )
        .await
        .unwrap();
    assert_eq!(diagnostics.content_integrity.io_skipped_file_count, Some(1));
    assert_eq!(
        store
            .code_index_task_queue_status()
            .await
            .unwrap()
            .dead_letter_task_count,
        0
    );
    drop(lock);
    let repaired = service
        .index_code_repository(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap();
    assert_eq!(repaired.summary.indexed_file_count, 2);
    assert_eq!(repaired.summary.source_scope, queued.source_scope);
}

#[tokio::test]
async fn source_io_diagnostic_pagination_counts_all_skipped_files() {
    let fixture = Fixture::new(false);
    let (service, _) = service(&fixture).await;
    let _first = fixture.lock();
    let _second = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(fixture.0.join("src/B.java"))
        .unwrap();
    let partial = service
        .index_code_repository(request("HEAD", CodeIndexMode::Full), context())
        .await
        .unwrap();
    assert_eq!(partial.summary.indexed_file_count, 0);
    let first = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: selector(&partial.summary.resolved_commit_sha),
                limit: 1,
                cursor: None,
            },
            context(),
        )
        .await
        .unwrap();
    assert_eq!(first.content_integrity.io_skipped_file_count, Some(2));
    assert_eq!(first.diagnostics.len(), 1);
    let second = service
        .code_repository_diagnostics(
            CodeDiagnosticsRequest {
                repository: selector(&partial.summary.resolved_commit_sha),
                limit: 1,
                cursor: Some(first.next_cursor.unwrap()),
            },
            context(),
        )
        .await
        .unwrap();
    assert_eq!(second.content_integrity.io_skipped_file_count, Some(2));
    assert_eq!(second.diagnostics.len(), 1);
    assert_ne!(first.diagnostics[0].path, second.diagnostics[0].path);
    assert!(second.next_cursor.is_none());
}
