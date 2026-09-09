//! Explicit Git refresh rejects aggregate origin work without retiring its base.
use super::*;

#[tokio::test]
async fn explicit_origin_refresh_rejects_total_bytes_and_preserves_base_queries() {
    let repo = FixtureRepo::create("incremental-origin-total-budget");
    let prefix = format!("{APP}#");
    let source = format!("{}{}\n", prefix, "x".repeat(256 * 1024 - prefix.len() - 1));
    for index in 0..64 {
        repo.write(&format!("app{index}.py"), &source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Bounded consumers"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    register_origin_repo(&service, &repo, vec![]).await;
    index_origin(&service, CodeIndexMode::Full, "HEAD", false).await;
    assert_origin_call(&service, &base, true).await;
    repo.write("typing.py", LOCAL);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Provider changes origin"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);
    let error = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::incremental(base.clone(), head).unwrap(),
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("origin-incremental-overflow"),
        )
        .await
        .unwrap_err();
    assert!(
        error.message.contains("bounded file/byte budget"),
        "{error:?}"
    );
    assert_origin_call(&service, &base, true).await;
}

#[tokio::test]
async fn changed_gitlink_consumers_are_parsed_once_when_provider_origin_changes() {
    for count in [2, 257] {
        let dependency = FixtureRepo::create("origin-gitlink-dependency");
        for index in 0..count {
            dependency.write(
                &format!("app{index}.py"),
                &format!("import typing\n@typing.overload\ndef pick{index}(x:int): ...\ndef pick{index}(x): return x\n"),
            );
        }
        dependency.git(["add", "."]);
        dependency.git(["commit", "-m", "Consumer base"]);
        let repo = FixtureRepo::create("origin-gitlink-root");
        repo.git(["clone", dependency.path.to_str().unwrap(), "pkg"]);
        repo.git(["config", "-f", ".gitmodules", "submodule.pkg.path", "pkg"]);
        repo.git([
            "config",
            "-f",
            ".gitmodules",
            "submodule.pkg.url",
            dependency.path.to_str().unwrap(),
        ]);
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "Gitlink base"]);
        let base = repo.git_text(["rev-parse", "HEAD"]);
        let service = service_with_memory_store().await;
        register_origin_repo(&service, &repo, vec![]).await;
        index_origin(&service, CodeIndexMode::Full, "HEAD", false).await;
        let child = FixtureRepo {
            path: repo.path.join("pkg"),
        };
        child.git(["config", "user.email", "relay@example.invalid"]);
        child.git(["config", "user.name", "Relay Test"]);
        child.write(
            "app0.py",
            "import typing\n@typing.overload\ndef pick0(x:int): ...\ndef pick0(x): return x+1\n",
        );
        child.git(["add", "."]);
        child.git(["commit", "-m", "Changed consumers"]);
        repo.write("typing.py", LOCAL);
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "Provider and gitlink"]);
        let head = repo.git_text(["rev-parse", "HEAD"]);
        let summary = index_origin(
            &service,
            CodeIndexMode::incremental(base.clone(), head).unwrap(),
            "HEAD",
            false,
        )
        .await;
        assert_eq!(summary.progress.parsed_file_count, count + 1);
        assert_eq!(summary.degraded_file_count, 0);
        for index in 0..count {
            let scope = relay_knowledge::domain::CodeRepositorySelector::new(
                "fixture",
                "HEAD",
                vec![format!("pkg/app{index}.py")],
                vec![],
            )
            .unwrap();
            let result = service
                .query_code_repository(
                    CodeRetrievalRequest::new(
                        format!("pick{index}"),
                        scope.clone(),
                        CodeQueryKind::Definition,
                        10,
                        FreshnessPolicy::WaitUntilFresh,
                    )
                    .unwrap(),
                    context("unique-gitlink-record"),
                )
                .await
                .unwrap();
            assert_eq!(result.results.len(), 2);
            assert!(!result.scope.stale);
            let canonical = result.results[0].canonical_symbol_id.as_ref().unwrap();
            let error = service
                .query_code_repository(
                    CodeRetrievalRequest::new(
                        canonical,
                        scope,
                        CodeQueryKind::Callees,
                        10,
                        FreshnessPolicy::WaitUntilFresh,
                    )
                    .unwrap(),
                    context("all-consumers-use-local-provider"),
                )
                .await
                .unwrap_err();
            assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
            if index == count - 1 {
                let old = service
                    .query_code_repository(
                        CodeRetrievalRequest::new(
                            canonical,
                            selector("fixture", &base),
                            CodeQueryKind::Callees,
                            10,
                            FreshnessPolicy::WaitUntilFresh,
                        )
                        .unwrap(),
                        context("old-gitlink-provider-proof"),
                    )
                    .await
                    .unwrap();
                assert!(!old.scope.stale);
            }
        }
    }
}
