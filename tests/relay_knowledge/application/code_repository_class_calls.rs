//! Issue #388: class-name calls must aggregate member edges in both directions.
use super::*;

#[tokio::test]
async fn java_class_call_queries_return_member_edges_without_reverse_text_matches() {
    let repo = FixtureRepo::create("java-class-calls");
    repo.write(
        "src/demo/A.java",
        "package demo; public class A { public static void main(String[] args) { B.process(); } }",
    );
    repo.write("src/demo/B.java", "package demo; public class B { public static void process() { System.out.println(\"processed\"); } }");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "class calls"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-java-class").await;
    let indexed = service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-java-class"),
        )
        .await
        .unwrap();
    assert_eq!(indexed.summary.indexed_file_count, 2);
    assert_eq!(indexed.summary.degraded_file_count, 0);
    let callers = query(&service, "B", CodeQueryKind::Callers).await;
    assert_eq!(callers.results.len(), 1);
    assert!(callers.results[0].excerpt.contains("main calls process"));
    assert_eq!(
        callers.results[0].edge_resolution_state.as_deref(),
        Some("resolved")
    );
    let callees = query(&service, "B", CodeQueryKind::Callees).await;
    assert_eq!(callees.results.len(), 1);
    assert!(callees.results[0].excerpt.contains("process calls println"));
    assert_eq!(
        callees.results[0].edge_resolution_state.as_deref(),
        Some("unresolved")
    );
    assert!(
        query(&service, "A", CodeQueryKind::Callers)
            .await
            .results
            .is_empty()
    );
    assert_eq!(
        query(&service, "A", CodeQueryKind::Callees)
            .await
            .results
            .len(),
        1
    );
    assert_eq!(
        query(&service, "B.process", CodeQueryKind::Callers)
            .await
            .results
            .len(),
        1
    );
    assert_eq!(
        query(&service, "process", CodeQueryKind::Callers)
            .await
            .results
            .len(),
        1
    );
    assert!(
        query(&service, "main", CodeQueryKind::Callers)
            .await
            .results
            .is_empty()
    );
    assert!(
        !query(&service, "B", CodeQueryKind::Definition)
            .await
            .results
            .is_empty()
    );
}
