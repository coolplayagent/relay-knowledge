//! Canonical Java method identities round-trip through the indexed call graph.
use super::*;

#[tokio::test]
async fn java_canonical_call_queries_preserve_exact_repository_and_class_identity() {
    let repo = FixtureRepo::create("java-canonical-calls");
    repo.write(
        "src/A.java",
        "package demo; public class A { public void run() { B.process(); } }",
    );
    repo.write(
        "src/B.java",
        "package demo; public class B { public static void process() {} }",
    );
    repo.write(
        "src/other/A.java",
        "package other; public class A { public void run() {} }",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Java method calls"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["src".to_owned()],
                language_filters: Vec::new(),
            },
            context("register"),
        )
        .await
        .expect("register Java fixture");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index"),
        )
        .await
        .expect("index Java fixture");

    for (name, path, kind, expected_path) in [
        (
            "process",
            "src/B.java",
            CodeQueryKind::Callers,
            "src/A.java",
        ),
        ("run", "src/A.java", CodeQueryKind::Callees, "src/A.java"),
    ] {
        let definitions = query(&service, name, CodeQueryKind::Definition).await;
        let canonical = definitions
            .results
            .iter()
            .filter(|hit| hit.path == path)
            .filter_map(|hit| hit.canonical_symbol_id.as_deref())
            .find(|id| id.ends_with(&format!(".{name}")))
            .expect("method definition exposes canonical ID");
        let answer = query(&service, canonical, kind).await;
        assert_eq!(answer.results.len(), 1, "{canonical}: {:?}", answer.results);
        assert_eq!(answer.results[0].path, expected_path);
        assert!(
            answer.results[0]
                .retrieval_layers
                .contains(&CodeRetrievalLayer::CallGraph)
        );
        let wrong_repository = format!(
            "repo://wrong/{}",
            canonical
                .strip_prefix("repo://")
                .unwrap()
                .split_once('/')
                .unwrap()
                .1
        );
        assert!(
            query(&service, &wrong_repository, kind)
                .await
                .results
                .is_empty()
        );
        let wrong_module = canonical.replace("src::", "absent::");
        assert!(
            query(&service, &wrong_module, kind)
                .await
                .results
                .is_empty()
        );
    }
    let definitions = query(&service, "run", CodeQueryKind::Definition).await;
    let other = definitions
        .results
        .iter()
        .filter(|hit| hit.path == "src/other/A.java")
        .filter_map(|hit| hit.canonical_symbol_id.as_deref())
        .find(|id| id.ends_with(".run"))
        .expect("other class method");
    assert!(
        query(&service, other, CodeQueryKind::Callees)
            .await
            .results
            .is_empty()
    );
}
