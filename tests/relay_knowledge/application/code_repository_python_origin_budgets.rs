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
