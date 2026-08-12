use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;

fn test_config() -> WatcherConfig {
    WatcherConfig {
        enabled: true,
        debounce: Duration::from_millis(100),
        commit_reconcile_interval: Duration::from_secs(60),
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
        last_indexed_commit: "commit-base".to_owned(),
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

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
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
fn should_process_path_rejects_path_outside_all_repos() {
    let state = WatcherInternalState {
        repositories: vec![test_repo("test")],
        hash_cache: ContentHashCache::new(1024),
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
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
        last_indexed_commit: "commit-base".to_owned(),
    };
    let state = WatcherInternalState {
        repositories: vec![repo],
        hash_cache: ContentHashCache::new(1024),
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
    };
    assert!(should_process_path(
        &state,
        &PathBuf::from("/tmp/test-watcher/src/main.rs")
    ));
}

#[test]
fn should_process_path_accepts_git_ref_hints_without_treating_git_as_source() {
    let state = WatcherInternalState {
        repositories: vec![test_repo("git-hint")],
        hash_cache: ContentHashCache::new(16),
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
    };

    assert!(should_process_path(
        &state,
        &PathBuf::from("/tmp/test-watcher/.git/HEAD")
    ));
    assert!(should_process_path(
        &state,
        &PathBuf::from("/tmp/test-watcher/.git/refs/heads/main")
    ));
    assert!(!should_process_path(
        &state,
        &PathBuf::from("/tmp/test-watcher/.git/index")
    ));
}

#[tokio::test]
async fn commit_reconciliation_queues_an_immutable_incremental_pair() {
    let root = temp_dir("git-reconcile");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "relay@example.test"]);
    git(&root, &["config", "user.name", "Relay Test"]);
    fs::write(root.join("lib.rs"), "pub fn first() {}\n").expect("first source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "first"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    fs::write(root.join("lib.rs"), "pub fn second() {}\n").expect("second source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "second"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root,
            last_indexed_commit: base.clone(),
            ..test_repo("git-reconcile")
        }],
        hash_cache: ContentHashCache::new(16),
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
    }));
    let (diag_tx, diag_rx) = watch::channel(WatcherDiagnostics::default());
    let dropped = Arc::new(AtomicU64::new(0));
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

    reconcile_all_commit_heads(&state, &diag_tx, &dropped, &sink).await;

    let queued = queued.lock().await;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].resolved_commit_sha, head);
    assert_eq!(
        queued[0].mode,
        crate::domain::CodeIndexMode::incremental(base, head).expect("mode")
    );
    assert_eq!(diag_rx.borrow().total_commit_reconciliations, 1);
    assert_eq!(diag_rx.borrow().total_commit_tasks_queued, 1);
}

#[tokio::test]
async fn unchanged_commit_reconciliation_skips_tree_scan_and_recovers_degraded_state() {
    let root = temp_dir("git-reconcile-unchanged");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "relay@example.test"]);
    git(&root, &["config", "user.name", "Relay Test"]);
    fs::write(root.join("lib.rs"), "pub fn unchanged() {}\n").expect("source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "unchanged"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    crate::code::reset_git_ls_tree_full_scan_call_count_for_root(root.clone());
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root: root.clone(),
            last_indexed_commit: head,
            ..test_repo("git-reconcile-unchanged")
        }],
        hash_cache: ContentHashCache::new(16),
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 1,
    }));
    let (diag_tx, diag_rx) = watch::channel(WatcherDiagnostics {
        state: WatcherState::Degraded,
        degraded_reason: Some("1 Git commit reconciliation attempt(s) failed".to_owned()),
        ..WatcherDiagnostics::default()
    });
    let dropped = Arc::new(AtomicU64::new(0));
    let sink: TaskQueueSink = Arc::new(|_| Box::pin(async { Ok(()) }));

    reconcile_all_commit_heads(&state, &diag_tx, &dropped, &sink).await;

    assert_eq!(diag_rx.borrow().state, WatcherState::Active);
    assert!(diag_rx.borrow().degraded_reason.is_none());
    assert_eq!(diag_rx.borrow().total_commit_reconciliations, 1);
    assert_eq!(
        crate::code::git_ls_tree_full_scan_call_count_for_root(&root),
        0
    );
}

#[tokio::test]
async fn periodic_reconciliation_tracks_stable_and_changed_worktree_observations() {
    let root = temp_dir("worktree-reconcile-observation");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "relay@example.test"]);
    git(&root, &["config", "user.name", "Relay Test"]);
    let source = root.join("lib.rs");
    fs::write(&source, "pub fn clean() {}\n").expect("clean source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "clean"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    fs::write(&source, "pub fn dirty_one() {}\n").expect("dirty source");
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root,
            last_indexed_commit: format!("worktree:{head}:published-overlay"),
            ..test_repo("worktree-reconcile-observation")
        }],
        hash_cache: ContentHashCache::new(16),
        deferred_changes: ContentHashCache::new(16),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
    }));
    let (diag_tx, _) = watch::channel(WatcherDiagnostics::default());
    let dropped = Arc::new(AtomicU64::new(0));
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

    reconcile_all_commit_heads(&state, &diag_tx, &dropped, &sink).await;
    reconcile_all_commit_heads(&state, &diag_tx, &dropped, &sink).await;
    fs::write(&source, "pub fn dirty_two() {}\n").expect("changed dirty source");
    reconcile_all_commit_heads(&state, &diag_tx, &dropped, &sink).await;

    let queued = queued.lock().await;
    assert_eq!(queued.len(), 3);
    assert_eq!(queued[0].input_fingerprint, queued[1].input_fingerprint);
    assert_ne!(queued[1].input_fingerprint, queued[2].input_fingerprint);
    assert!(
        queued
            .iter()
            .all(|seed| seed.mode == crate::domain::CodeIndexMode::WorktreeOverlay)
    );
}

#[tokio::test]
async fn deferred_dirty_event_replays_after_commit_publication_updates_the_clean_base() {
    let root = temp_dir("deferred-after-commit");
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "relay@example.test"]);
    git(&root, &["config", "user.name", "Relay Test"]);
    let source = root.join("lib.rs");
    fs::write(&source, "pub fn base() {}\n").expect("base source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    fs::write(&source, "pub fn committed() {}\n").expect("committed source");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "head"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    fs::write(&source, "pub fn dirty_after_head() {}\n").expect("dirty source");
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![WatchedRepository {
            root,
            last_indexed_commit: base,
            ..test_repo("deferred-after-commit")
        }],
        hash_cache: ContentHashCache::new(16),
        deferred_changes: ContentHashCache::new(16),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
    }));
    let (diag_tx, _) = watch::channel(WatcherDiagnostics::default());
    let dropped = Arc::new(AtomicU64::new(0));
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
                    return Err("commit update still owns the repository writer".to_owned());
                }
                queued.lock().await.push(seed);
                Ok(())
            })
        })
    };

    process_debounced_paths(
        &state,
        &diag_tx,
        &dropped,
        std::slice::from_ref(&source),
        &sink,
    )
    .await;
    assert_eq!(state.read().await.deferred_changes.snapshots().len(), 1);
    state.write().await.repositories[0].last_indexed_commit = head.clone();

    reconcile_all_commit_heads(&state, &diag_tx, &dropped, &sink).await;

    assert!(state.read().await.deferred_changes.snapshots().is_empty());
    let queued = queued.lock().await;
    assert_eq!(queued.len(), 2);
    assert!(queued.iter().all(|seed| seed.ref_selector == head));
    assert!(
        queued
            .iter()
            .all(|seed| seed.mode == crate::domain::CodeIndexMode::WorktreeOverlay)
    );
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
async fn active_handle_preserves_same_root_repository_scopes() {
    let root = temp_dir("same-root-scopes");
    let primary = WatchedRepository {
        root: root.clone(),
        path_filters: vec!["src".to_owned()],
        ..test_repo("same-root-primary")
    };
    let secondary = WatchedRepository {
        root: root.clone(),
        path_filters: vec!["docs".to_owned()],
        ..test_repo("same-root-secondary")
    };
    let handle = FileWatcher::new(test_config())
        .start(vec![])
        .expect("handle");

    assert!(handle.add_repository(primary).await);
    assert!(handle.add_repository(secondary).await);
    assert_eq!(handle.repository_count().await, 2);

    assert!(handle.remove_repository("same-root-primary").await);
    let state = handle.state.read().await;
    assert_eq!(state.repositories.len(), 1);
    assert_eq!(state.repositories[0].alias, "same-root-secondary");
    assert_eq!(state.repositories[0].root, root);
    drop(state);

    assert!(handle.remove_repository("same-root-secondary").await);
    assert_eq!(handle.repository_count().await, 0);
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
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
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
async fn process_debounced_paths_queues_only_matching_same_root_scopes() {
    let root = temp_dir("queue-same-root-filters");
    let docs = root.join("docs");
    fs::create_dir_all(&docs).expect("docs dir");
    let changed = docs.join("README.md");
    fs::write(&changed, "# notes\n").expect("changed file");
    let state = Arc::new(RwLock::new(WatcherInternalState {
        repositories: vec![
            WatchedRepository {
                root: root.clone(),
                path_filters: vec!["src".to_owned()],
                ..test_repo("same-root-src")
            },
            WatchedRepository {
                root: root.clone(),
                path_filters: vec!["docs".to_owned()],
                ..test_repo("same-root-docs")
            },
        ],
        hash_cache: ContentHashCache::new(1024),
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
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

    process_debounced_paths(&state, &diag_tx, &dropped_events, &[changed], &sink).await;

    let queued = queued.lock().await;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].alias, "same-root-docs");
    assert_eq!(queued[0].path_filters, ["docs"]);
    assert_eq!(state.read().await.index_tasks_queued, 1);
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
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
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
    assert_eq!(queued[0].tree_hash, queued[1].tree_hash);
    assert_eq!(queued[0].source_scope, queued[1].source_scope);
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
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
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
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
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
            deferred_changes: ContentHashCache::new(1024),
            events_received: 0,
            events_filtered: 0,
            index_tasks_queued: 0,
            commit_reconciliations: 0,
            commit_tasks_queued: 0,
            commit_reconcile_failures: 0,
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
        deferred_changes: ContentHashCache::new(1024),
        events_received: 0,
        events_filtered: 0,
        index_tasks_queued: 0,
        commit_reconciliations: 0,
        commit_tasks_queued: 0,
        commit_reconcile_failures: 0,
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
        total_commit_reconciliations: 0,
        total_commit_tasks_queued: 0,
        total_commit_reconcile_failures: 0,
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
        total_commit_reconciliations: 0,
        total_commit_tasks_queued: 0,
        total_commit_reconcile_failures: 0,
        total_events_dropped: 0,
        last_error: Some("test error".to_owned()),
        degraded_reason: None,
    };
    let json = serde_json::to_string(&diag).expect("serialize");
    let parsed: WatcherDiagnostics = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, diag);
}
