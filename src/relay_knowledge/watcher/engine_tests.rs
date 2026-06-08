use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

fn test_config() -> WatcherConfig {
    WatcherConfig {
        enabled: true,
        debounce: Duration::from_millis(100),
        max_watch_dirs: 1024,
        hash_cache_capacity: 1024,
    }
}

fn test_repo(alias: &str) -> WatchedRepository {
    WatchedRepository {
        repository_id: format!("repo-{alias}"),
        alias: alias.to_owned(),
        root: PathBuf::from("/tmp/test-watcher"),
        path_filters: vec![],
        language_filters: vec![],
        source_scope: format!("scope-{alias}"),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "relay-knowledge-watcher-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir");
    path
}

#[test]
fn watcher_state_roundtrip() {
    for state in [
        WatcherState::Disabled,
        WatcherState::Active,
        WatcherState::Degraded,
        WatcherState::Failed,
    ] {
        assert_eq!(WatcherState::parse(state.as_str()), Some(state));
    }
}

#[test]
fn watcher_state_parse_unknown() {
    assert_eq!(WatcherState::parse("unknown"), None);
}

#[test]
fn disabled_watcher_returns_disabled_handle() {
    let config = WatcherConfig {
        enabled: false,
        ..test_config()
    };
    let watcher = FileWatcher::new(config);
    let handle = watcher
        .start(vec![])
        .expect("disabled watcher should succeed");
    assert_eq!(handle.diagnostics().state, WatcherState::Disabled);
}

#[tokio::test]
async fn disabled_handle_rejects_dynamic_repository_changes() {
    let config = WatcherConfig {
        enabled: false,
        ..test_config()
    };
    let handle = FileWatcher::new(config).start(vec![]).expect("handle");

    assert!(!handle.add_repository(test_repo("r1")).await);
    assert!(!handle.remove_repository("r1").await);
    assert_eq!(handle.repository_count().await, 0);
}

#[test]
fn diagnostics_default_is_disabled() {
    let diag = WatcherDiagnostics::default();
    assert_eq!(diag.state, WatcherState::Disabled);
    assert_eq!(diag.watched_repository_count, 0);
    assert_eq!(diag.total_events_received, 0);
    assert!(diag.last_error.is_none());
}

#[test]
fn build_incremental_task_seed_returns_none_for_empty_paths() {
    let repo = test_repo("test");
    let seed = build_incremental_task_seed(&repo, &[], "HEAD", "abc123", "tree1", 0xabc, 1000);
    assert!(seed.is_none());
}

#[test]
fn build_incremental_task_seed_returns_valid_overlay_seed() {
    let repo = test_repo("test");
    let paths = vec![PathBuf::from("/tmp/test-watcher/src/main.rs")];
    let seed = build_incremental_task_seed(&repo, &paths, "HEAD", "sha123", "tree456", 0xabc, 1000)
        .expect("should return seed");
    assert_eq!(seed.repository_id, "repo-test");
    assert_eq!(seed.alias, "test");
    assert_eq!(seed.ref_selector, "HEAD");
    assert_eq!(seed.resolved_commit_sha, "sha123");
    assert_eq!(seed.tree_hash, "tree456");
    assert!(seed.input_fingerprint.starts_with("worktree_overlay:"));
    assert_eq!(seed.now_ms, 1000);
}

#[test]
fn build_incremental_task_seed_payload_is_code_index_request() {
    let repo = test_repo("payload");
    let paths = vec![PathBuf::from("/tmp/test-watcher/x.rs")];
    let seed = build_incremental_task_seed(&repo, &paths, "HEAD", "", "", 0xabc, 0).unwrap();
    let request: crate::domain::CodeIndexRequest =
        serde_json::from_str(&seed.payload_json).expect("payload should be CodeIndexRequest");
    assert_eq!(request.repository.repository, "payload");
    assert_eq!(request.repository.ref_selector, "HEAD");
    assert_eq!(request.mode, crate::domain::CodeIndexMode::WorktreeOverlay);
}

#[test]
fn build_incremental_task_seed_fingerprint_includes_path_set() {
    let repo = test_repo("fp");
    let paths1 = vec![PathBuf::from("/tmp/test-watcher/a.rs")];
    let paths2 = vec![PathBuf::from("/tmp/test-watcher/b.rs")];
    let seed1 =
        build_incremental_task_seed(&repo, &paths1, "HEAD", "sha", "tree", 0xabc, 0).unwrap();
    let seed2 =
        build_incremental_task_seed(&repo, &paths2, "HEAD", "sha", "tree", 0xabc, 0).unwrap();
    assert_ne!(seed1.input_fingerprint, seed2.input_fingerprint);
}

#[test]
fn build_incremental_task_seed_fingerprint_includes_content_generation() {
    let repo = test_repo("content-fp");
    let paths = vec![PathBuf::from("/tmp/test-watcher/a.rs")];
    let seed1 = build_incremental_task_seed(&repo, &paths, "HEAD", "sha", "", 0xabc, 0).unwrap();
    let seed2 = build_incremental_task_seed(&repo, &paths, "HEAD", "sha", "", 0xdef, 0).unwrap();
    assert_ne!(seed1.tree_hash, seed2.tree_hash);
    assert_ne!(seed1.input_fingerprint, seed2.input_fingerprint);
    assert!(seed1.payload_json.contains("\"content_fingerprint\""));
}

#[test]
fn should_process_path_rejects_path_outside_all_repos() {
    let state = WatcherInternalState {
        repositories: vec![test_repo("test")],
        hash_cache: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
    };
    assert!(!should_process_path(
        &state,
        &PathBuf::from("/other/project/main.rs")
    ));
}

#[test]
fn should_process_path_accepts_matching_file_in_repo() {
    let repo = WatchedRepository {
        repository_id: "repo-test".to_owned(),
        alias: "test".to_owned(),
        root: PathBuf::from("/tmp/test-watcher"),
        path_filters: vec![],
        language_filters: vec![],
        source_scope: "scope-test".to_owned(),
    };
    let state = WatcherInternalState {
        repositories: vec![repo],
        hash_cache: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
    };
    assert!(should_process_path(
        &state,
        &PathBuf::from("/tmp/test-watcher/src/main.rs")
    ));
}

#[tokio::test]
async fn active_handle_watches_and_unwatches_dynamic_repository() {
    let root = temp_dir("dynamic");
    let repo = WatchedRepository {
        root,
        ..test_repo("dynamic")
    };
    let handle = FileWatcher::new(test_config())
        .start(vec![])
        .expect("handle");

    assert!(handle.add_repository(repo).await);
    assert_eq!(handle.repository_count().await, 1);
    assert!(!handle.add_repository(test_repo("dynamic")).await);
    assert!(handle.remove_repository("dynamic").await);
    assert_eq!(handle.repository_count().await, 0);
    assert!(!handle.remove_repository("dynamic").await);
    handle.request_shutdown();
}

#[tokio::test]
async fn active_handle_refreshes_existing_repository_registration() {
    let root = temp_dir("refresh");
    let repo = WatchedRepository {
        root: root.clone(),
        path_filters: vec!["src".to_owned()],
        source_scope: "scope-old".to_owned(),
        ..test_repo("refresh")
    };
    let handle = FileWatcher::new(test_config())
        .start(vec![])
        .expect("handle");

    assert!(handle.add_repository(repo).await);
    let refreshed = WatchedRepository {
        root,
        path_filters: vec!["crates".to_owned()],
        source_scope: "scope-new".to_owned(),
        ..test_repo("refresh")
    };
    assert!(handle.add_repository(refreshed).await);

    let state = handle.state.read().await;
    assert_eq!(state.repositories.len(), 1);
    assert_eq!(state.repositories[0].path_filters, vec!["crates"]);
    assert_eq!(state.repositories[0].source_scope, "scope-new");
    drop(state);
    handle.request_shutdown();
}

#[tokio::test]
async fn active_handle_refreshes_repository_root() {
    let old_root = temp_dir("refresh-root-old");
    let new_root = temp_dir("refresh-root-new");
    let repo = WatchedRepository {
        root: old_root,
        ..test_repo("refresh-root")
    };
    let handle = FileWatcher::new(test_config())
        .start(vec![])
        .expect("handle");

    assert!(handle.add_repository(repo).await);
    assert!(
        handle
            .add_repository(WatchedRepository {
                root: new_root.clone(),
                source_scope: "scope-refresh-root-new".to_owned(),
                ..test_repo("refresh-root")
            })
            .await
    );

    let state = handle.state.read().await;
    assert_eq!(state.repositories.len(), 1);
    assert_eq!(state.repositories[0].root, new_root);
    assert_eq!(state.repositories[0].source_scope, "scope-refresh-root-new");
    drop(state);
    handle.request_shutdown();
}

#[tokio::test]
async fn process_debounced_paths_queues_one_task_per_repository() {
    let root = temp_dir("queue");
    let src = root.join("src");
    fs::create_dir_all(&src).expect("src dir");
    let changed = src.join("main.rs");
    fs::write(&changed, "fn main() {}\n").expect("changed file");
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root: root.clone(),
            ..test_repo("queue")
        }],
        hash_cache: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
    }));
    let (diag_tx, diag_rx) = watch::channel(WatcherDiagnostics::default());
    let dropped_events = Arc::new(AtomicU64::new(0));
    let queued = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let sink: TaskQueueSink = {
        let queued = Arc::clone(&queued);
        Arc::new(move |seed| {
            let queued = Arc::clone(&queued);
            Box::pin(async move {
                queued.lock().await.push(seed);
                Ok(())
            })
        })
    };

    process_debounced_paths(&state, &diag_tx, &dropped_events, &[changed], &sink).await;

    assert_eq!(queued.lock().await.len(), 1);
    assert_eq!(state.read().await.index_tasks_queued, 1);
    assert_eq!(diag_rx.borrow().total_index_tasks_queued, 1);
}

#[tokio::test]
async fn process_debounced_paths_uses_content_generation_in_task_fingerprint() {
    let root = temp_dir("content-generation");
    let changed = root.join("main.rs");
    fs::write(&changed, "fn main() {}\n").expect("changed file");
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root,
            ..test_repo("content-generation")
        }],
        hash_cache: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
    }));
    let (diag_tx, _) = watch::channel(WatcherDiagnostics::default());
    let dropped_events = Arc::new(AtomicU64::new(0));
    let queued = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let sink: TaskQueueSink = {
        let queued = Arc::clone(&queued);
        Arc::new(move |seed| {
            let queued = Arc::clone(&queued);
            Box::pin(async move {
                queued.lock().await.push(seed);
                Ok(())
            })
        })
    };

    process_debounced_paths(
        &state,
        &diag_tx,
        &dropped_events,
        std::slice::from_ref(&changed),
        &sink,
    )
    .await;
    fs::write(&changed, "fn main() { println!(\"changed\"); }\n").expect("changed file");
    process_debounced_paths(&state, &diag_tx, &dropped_events, &[changed], &sink).await;

    let queued = queued.lock().await;
    assert_eq!(queued.len(), 2);
    assert_ne!(queued[0].tree_hash, queued[1].tree_hash);
    assert_ne!(queued[0].input_fingerprint, queued[1].input_fingerprint);
}

#[tokio::test]
async fn process_debounced_paths_filters_unchanged_hashes() {
    let root = temp_dir("hash-filter");
    let changed = root.join("main.rs");
    fs::write(&changed, "fn main() {}\n").expect("changed file");
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root,
            ..test_repo("hash-filter")
        }],
        hash_cache: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
    }));
    let (diag_tx, _) = watch::channel(WatcherDiagnostics::default());
    let dropped_events = Arc::new(AtomicU64::new(0));
    let sink: TaskQueueSink = Arc::new(|_| Box::pin(async { Ok(()) }));

    process_debounced_paths(
        &state,
        &diag_tx,
        &dropped_events,
        std::slice::from_ref(&changed),
        &sink,
    )
    .await;
    process_debounced_paths(&state, &diag_tx, &dropped_events, &[changed], &sink).await;

    let state = state.read().await;
    assert_eq!(state.index_tasks_queued, 1);
    assert_eq!(state.events_filtered, 1);
}

#[tokio::test]
async fn process_debounced_paths_retries_same_content_after_queue_failure() {
    let root = temp_dir("queue-failure-retry");
    let changed = root.join("main.rs");
    fs::write(&changed, "fn main() {}\n").expect("changed file");
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root,
            ..test_repo("queue-failure-retry")
        }],
        hash_cache: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
    }));
    let (diag_tx, _) = watch::channel(WatcherDiagnostics::default());
    let dropped_events = Arc::new(AtomicU64::new(0));
    let attempts = Arc::new(AtomicUsize::new(0));
    let queued = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let sink: TaskQueueSink = {
        let attempts = Arc::clone(&attempts);
        let queued = Arc::clone(&queued);
        Arc::new(move |seed| {
            let attempts = Arc::clone(&attempts);
            let queued = Arc::clone(&queued);
            Box::pin(async move {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err("temporary queue failure".to_owned());
                }
                queued.lock().await.push(seed);
                Ok(())
            })
        })
    };

    process_debounced_paths(
        &state,
        &diag_tx,
        &dropped_events,
        std::slice::from_ref(&changed),
        &sink,
    )
    .await;
    process_debounced_paths(
        &state,
        &diag_tx,
        &dropped_events,
        std::slice::from_ref(&changed),
        &sink,
    )
    .await;
    process_debounced_paths(&state, &diag_tx, &dropped_events, &[changed], &sink).await;

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(queued.lock().await.len(), 1);
    let state = state.read().await;
    assert_eq!(state.index_tasks_queued, 1);
    assert_eq!(state.events_filtered, 1);
}

#[tokio::test]
async fn handle_returns_false_when_command_loop_is_unavailable() {
    let (_diag_tx, diag_rx) = watch::channel(WatcherDiagnostics {
        state: WatcherState::Active,
        ..WatcherDiagnostics::default()
    });
    let (shutdown_tx, _) = watch::channel(false);
    let (command_tx, command_rx) = mpsc::channel(1);
    drop(command_rx);
    let handle = WatcherHandle {
        diagnostics: diag_rx,
        shutdown: shutdown_tx,
        state: Arc::new(RwLock::new(WatcherInternalState {
            repositories: Vec::new(),
            hash_cache: ContentHashCache::new(1024),
            events_received: 0,
            events_filtered: 0,
            index_tasks_queued: 0,
        })),
        command_tx: Some(command_tx),
    };

    assert!(!handle.add_repository(test_repo("closed")).await);
    assert!(!handle.remove_repository("closed").await);
}

#[test]
fn dropped_event_counter_flows_into_diagnostics() {
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: Vec::new(),
        hash_cache: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
    }));
    let (diag_tx, diag_rx) = watch::channel(WatcherDiagnostics::default());
    let dropped_events = Arc::new(AtomicU64::new(0));
    dropped_events.fetch_add(3, Ordering::Relaxed);

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(emit_diagnostics(&state, &diag_tx, &dropped_events));

    assert_eq!(diag_rx.borrow().total_events_dropped, 3);
}

#[test]
fn degraded_diagnostics_preserves_counts() {
    let diag = WatcherDiagnostics {
        state: WatcherState::Active,
        watched_repository_count: 3,
        total_events_received: 100,
        total_events_filtered: 20,
        total_index_tasks_queued: 80,
        total_events_dropped: 0,
        last_error: None,
        degraded_reason: None,
    };
    let updated = WatcherDiagnostics {
        state: WatcherState::Degraded,
        degraded_reason: Some("limit exceeded".to_owned()),
        ..diag.clone()
    };
    assert_eq!(updated.watched_repository_count, 3);
    assert_eq!(updated.total_events_received, 100);
    assert_eq!(updated.total_index_tasks_queued, 80);
}

#[test]
fn watcher_diagnostics_serialization_roundtrip() {
    let diag = WatcherDiagnostics {
        state: WatcherState::Active,
        watched_repository_count: 5,
        total_events_received: 42,
        total_events_filtered: 10,
        total_index_tasks_queued: 32,
        total_events_dropped: 0,
        last_error: Some("test error".to_owned()),
        degraded_reason: None,
    };
    let json = serde_json::to_string(&diag).expect("serialize");
    let parsed: WatcherDiagnostics = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, diag);
}
