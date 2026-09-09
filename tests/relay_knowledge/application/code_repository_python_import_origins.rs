//! Source-scope module identity survives durable indexing and import-origin changes.
use super::*;

const APP: &str = "import typing\ndef leaf(): return 1\n@typing.overload\ndef pick(value:int): ...\ndef pick(value): return leaf()\n";
const LOCAL: &str = "def overload(function):\n return function\n";

#[tokio::test]
async fn explicit_python_incremental_provider_change_reparses_unchanged_app() {
    let repo = FixtureRepo::create("python-origin-explicit-incremental");
    repo.write("app.py", APP);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Base"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    register_origin_repo(&service, &repo, vec![]).await;
    index_origin(&service, CodeIndexMode::Full, "HEAD", false).await;
    repo.write("typing.py", LOCAL);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Add local"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);
    index_origin(
        &service,
        CodeIndexMode::incremental(base, head).unwrap(),
        "HEAD",
        false,
    )
    .await;
    assert_origin_call(&service, "HEAD", false).await;
}

async fn register_origin_repo(
    service: &RelayKnowledgeService,
    repo: &FixtureRepo,
    filters: Vec<String>,
) {
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: filters,
                language_filters: vec![],
            },
            context("register-python-origin"),
        )
        .await
        .unwrap();
}

async fn index_origin(
    service: &RelayKnowledgeService,
    mode: CodeIndexMode,
    reference: &str,
    reuse: bool,
) -> relay_knowledge::domain::CodeIndexSummary {
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", reference),
                mode,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: reuse,
            },
            context("index-python-origin"),
        )
        .await
        .unwrap()
        .summary
}

async fn assert_origin_call(service: &RelayKnowledgeService, reference: &str, typed: bool) {
    let definitions = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "pick",
                selector("fixture", reference),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("origin-definitions"),
        )
        .await
        .unwrap();
    assert!(!definitions.scope.stale);
    assert!(
        definitions
            .results
            .iter()
            .all(|hit| hit.degraded_reason.is_none())
    );
    let canonical = definitions
        .results
        .iter()
        .find_map(|hit| {
            hit.canonical_symbol_id
                .as_deref()
                .filter(|name| name.ends_with("::pick"))
        })
        .unwrap();
    let calls = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                canonical,
                selector("fixture", reference),
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("origin-callees"),
        )
        .await;
    if typed {
        assert_eq!(calls.unwrap().results.len(), 1);
    } else {
        assert_eq!(calls.unwrap_err().error_kind, ErrorKind::InvalidArgument);
    }
}

#[tokio::test]
async fn python_local_provider_add_delete_reindexes_unchanged_callers_and_retains_refs() {
    let repo = FixtureRepo::create("python-origin-lifecycle");
    repo.write("app.py", APP);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Standard provider"]);
    let standard = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    register_origin_repo(&service, &repo, vec![]).await;
    index_origin(&service, CodeIndexMode::Full, "HEAD", false).await;
    assert_origin_call(&service, "HEAD", true).await;
    repo.write("typing.py", LOCAL);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Local provider"]);
    let local = repo.git_text(["rev-parse", "HEAD"]);
    index_origin(&service, CodeIndexMode::Full, "HEAD", true).await;
    assert_origin_call(&service, "HEAD", false).await;
    assert_origin_call(&service, &standard, true).await;
    std::fs::remove_file(repo.path.join("typing.py")).unwrap();
    repo.write("README.md", "Deletion lifecycle\n");
    repo.git(["add", "-A"]);
    repo.git(["commit", "-m", "Remove local provider"]);
    index_origin(&service, CodeIndexMode::Full, "HEAD", true).await;
    assert_origin_call(&service, "HEAD", true).await;
    assert_origin_call(&service, &local, false).await;
    // The existing retention policy keeps two successful scopes. The earliest
    // standard scope must be retired rather than silently redirected to HEAD.
    let retired = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "pick",
                selector("fixture", &standard),
                CodeQueryKind::Definition,
                10,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("retired-origin-scope"),
        )
        .await
        .unwrap_err();
    assert_eq!(retired.error_kind, ErrorKind::InvalidArgument);
    assert!(retired.message.contains("has no index"));
}

#[tokio::test]
async fn python_overlay_provider_changes_reparse_unchanged_python_files() {
    let repo = FixtureRepo::create("python-origin-overlay");
    repo.write("app.py", APP);
    repo.write("README.md", "unchanged\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Standard provider"]);
    let service = service_with_memory_store().await;
    register_origin_repo(&service, &repo, vec![]).await;
    index_origin(&service, CodeIndexMode::Full, "HEAD", false).await;
    repo.write("app.py", "# staged\n");
    repo.write("README.md", "staged\n");
    repo.git(["add", "app.py", "README.md"]);
    repo.write("app.py", APP);
    repo.write("README.md", "unchanged\n");
    repo.write("typing.py", LOCAL);
    let overlay = index_origin(&service, CodeIndexMode::WorktreeOverlay, "HEAD", false).await;
    assert_eq!(overlay.progress.parsed_file_count, 2);
    assert_eq!(overlay.skipped_unchanged_count, 1);
    assert_eq!(overlay.changed_path_count, 3);
    assert_origin_call(&service, "worktree", false).await;
    assert_origin_call(&service, "HEAD", true).await;
    std::fs::remove_file(repo.path.join("typing.py")).unwrap();
    index_origin(&service, CodeIndexMode::WorktreeOverlay, "HEAD", false).await;
    assert_origin_call(&service, "worktree", true).await;
}

#[tokio::test]
async fn python_restricted_scope_does_not_guess_unseen_standard_module_identity() {
    let repo = FixtureRepo::create("python-origin-restricted");
    repo.write("src/app.py", APP);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Restricted origin"]);
    let service = service_with_memory_store().await;
    register_origin_repo(&service, &repo, vec!["src".into()]).await;
    index_origin(&service, CodeIndexMode::Full, "HEAD", false).await;
    assert_origin_call(&service, "HEAD", false).await;
}
