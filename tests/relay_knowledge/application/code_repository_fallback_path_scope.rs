//! Source fallback must narrow a registered scope before reading candidates.

use super::*;

#[tokio::test]
async fn hybrid_fallback_respects_nonexistent_and_sibling_request_prefixes() {
    let repo = FixtureRepo::create("fallback-request-path-scope");
    for path in [
        "docs/left/item.md",
        "docs/right/item.md",
        "docs/leftover/item.md",
    ] {
        repo.write(
            path,
            "# Scope fixture\n\nScopeNeedle appears in this source line.\n",
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "scope fixture"]);
    let output = git_command(&repo.path, ["rev-parse", "HEAD"])
        .output()
        .expect("Git should resolve fixture commit");
    assert!(output.status.success());
    let commit = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["docs".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-fallback-path-scope"),
        )
        .await
        .unwrap();
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", &commit),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-fallback-path-scope"),
        )
        .await
        .unwrap();

    for (paths, expected) in [
        (
            vec![],
            vec![
                "docs/left/item.md",
                "docs/right/item.md",
                "docs/leftover/item.md",
            ],
        ),
        (vec!["docs/missing/"], vec![]),
        (vec!["./docs/left/"], vec!["docs/left/item.md"]),
        (vec!["docs/left/item.md"], vec!["docs/left/item.md"]),
        (
            vec!["docs/left", "docs/right"],
            vec!["docs/left/item.md", "docs/right/item.md"],
        ),
    ] {
        let response = service
            .query_code_repository(
                CodeRetrievalRequest::new(
                    "ScopeNeedle",
                    CodeRepositorySelector::new(
                        "fixture",
                        &commit,
                        paths.iter().map(|path| (*path).to_owned()).collect(),
                        Vec::new(),
                    )
                    .unwrap(),
                    CodeQueryKind::Hybrid,
                    10,
                    FreshnessPolicy::AllowStale,
                )
                .unwrap(),
                context("query-fallback-path-scope"),
            )
            .await
            .expect("a narrower indexed scope should remain queryable");
        assert!(
            response.degraded_reason.is_none(),
            "narrowing is not degradation: {:?}",
            response.degraded_reason
        );
        let actual = response
            .results
            .iter()
            .map(|hit| hit.path.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            actual,
            expected.into_iter().collect(),
            "requested paths: {paths:?}"
        );
        assert!(
            response
                .results
                .iter()
                .all(|hit| !hit.query_degraded && hit.resolved_commit_sha == commit)
        );
    }
    let outside =
        CodeRepositorySelector::new("fixture", &commit, vec!["other".to_owned()], Vec::new())
            .unwrap();
    let error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                "ScopeNeedle",
                outside,
                CodeQueryKind::Hybrid,
                10,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("query-outside-registered-scope"),
        )
        .await
        .expect_err("a disjoint request must not reuse the registered scope");
    assert!(error.message.contains("requested filters"));
    fs::remove_dir_all(&repo.path).unwrap();
}
