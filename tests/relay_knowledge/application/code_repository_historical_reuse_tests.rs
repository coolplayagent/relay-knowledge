use super::*;

#[tokio::test]
async fn incremental_index_uses_persisted_base_scope_when_head_is_active() {
    let repo = FixtureRepo::create("code-incremental-base");
    repo.write("src/lib.rs", "pub fn value() -> u32 { 2 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "initial"]);
    let initial = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;

    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-incremental-base"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-incremental-base"),
        )
        .await
        .expect("initial index should succeed");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "update to one"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(initial.clone(), "HEAD")
                    .expect("incremental mode should validate"),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-current-base"),
        )
        .await
        .expect("first incremental index should succeed");

    repo.write("src/lib.rs", "pub fn value() -> u32 { 0 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "return to zero"]);
    let updated = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(initial.clone(), "HEAD")
                    .expect("incremental mode should validate"),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-persisted-base"),
        )
        .await
        .expect("persisted base scope should seed incremental update");

    assert_eq!(
        updated.summary.base_resolved_commit_sha.as_deref(),
        Some(initial.as_str())
    );
    assert_eq!(updated.summary.changed_path_count, 1);
    assert_eq!(updated.summary.progress.blob_read_count, 1);
    assert!(
        query(&service, "value", CodeQueryKind::Definition)
            .await
            .results
            .iter()
            .any(|hit| hit.path == "src/lib.rs")
    );
}

#[tokio::test]
async fn full_index_with_historical_reuse_uses_nearest_indexed_first_parent() {
    let repo = FixtureRepo::create("code-full-history-reuse");
    repo.write("src/lib.rs", "pub fn historical_value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-full-history-reuse").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-full-history-base"),
        )
        .await
        .expect("base index should succeed");

    for value in 2..=11 {
        repo.write(
            "src/lib.rs",
            &format!("pub fn historical_value() -> u32 {{ {value} }}\n"),
        );
        repo.git(["add", "."]);
        repo.git(["commit", "-m", &format!("advance-{value}")]);
    }
    let head = repo.git_text(["rev-parse", "HEAD"]);
    let request = CodeIndexRequest {
        repository: selector("fixture", "HEAD"),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        reuse_historical: true,
    };
    let first_start = service
        .start_code_repository_index(request.clone(), context("start-full-history-reuse"))
        .await
        .expect("full request should queue from its indexed ancestor");
    let first_task = first_start.task.expect("cold target should return a task");
    assert_eq!(
        first_task.mode,
        CodeIndexMode::incremental(base.clone(), head.clone())
            .expect("pinned refs should validate")
    );
    let duplicate_start = service
        .start_code_repository_index(request.clone(), context("repeat-full-history-reuse"))
        .await
        .expect("duplicate full request should reuse the target task");
    assert_eq!(
        duplicate_start.task.as_ref().map(|task| &task.task_id),
        Some(&first_task.task_id)
    );

    let completed = service
        .index_code_repository(request, context("run-full-history-reuse"))
        .await
        .expect("opt-in incremental task should run");

    assert_eq!(
        completed.summary.base_resolved_commit_sha.as_deref(),
        Some(base.as_str())
    );
    assert_eq!(completed.summary.resolved_commit_sha, head);
    assert_eq!(completed.summary.changed_path_count, 1);
    assert_eq!(completed.summary.progress.blob_read_count, 1);
    assert_eq!(completed.summary.progress.parsed_file_count, 1);
    assert!(
        query(&service, "historical_value", CodeQueryKind::Definition)
            .await
            .results
            .iter()
            .any(|hit| hit.excerpt.contains("{ 11 }"))
    );
}

#[tokio::test]
async fn full_index_falls_back_when_ancestor_delta_exceeds_budget() {
    let repo = FixtureRepo::create("code-full-history-budget");
    repo.write("src/lib.rs", "pub fn stable_value() -> u32 { 1 }\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "base"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-full-history-budget").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-full-history-budget-base"),
        )
        .await
        .expect("base index should succeed");
    for index in 0..101 {
        repo.write(&format!("noise/file-{index:04}.txt"), "noise\n");
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "oversized delta"]);

    let started = service
        .start_code_repository_index(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: true,
            },
            context("start-full-history-budget"),
        )
        .await
        .expect("oversized historical delta should fall back to full indexing");

    assert_eq!(
        started
            .task
            .expect("full fallback should queue a task")
            .mode,
        CodeIndexMode::Full
    );
}
