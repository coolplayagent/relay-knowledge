//! Canonical Java method identities round-trip through the indexed call graph.
use super::*;

#[tokio::test]
async fn python_overload_stubs_select_the_runtime_implementation() {
    let repo = FixtureRepo::create("python-overload-calls");
    repo.write("src/sample.py", "import typing\n@typing.overload\ndef choose(value: int): ...\n@typing.overload\ndef choose(value: str): ...\ndef choose(value): return leaf(value)\ndef leaf(value): return value\ndef caller(): return choose(1)\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Python overload declarations"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-python-overloads"),
        )
        .await
        .unwrap();
    let definitions = query(&service, "choose", CodeQueryKind::Definition).await;
    let canonical = definitions
        .results
        .iter()
        .find_map(|hit| {
            hit.canonical_symbol_id
                .as_deref()
                .filter(|id| id.ends_with("::choose"))
        })
        .unwrap();
    for (kind, expected) in [
        (CodeQueryKind::Callers, "::caller"),
        (CodeQueryKind::Callees, "::leaf"),
    ] {
        let response = query(&service, canonical, kind).await;
        assert_eq!(response.results.len(), 1);
        assert!(
            response.results[0]
                .canonical_symbol_id
                .as_deref()
                .unwrap()
                .ends_with(expected)
        );
    }
}

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

#[tokio::test]
async fn canonical_inline_filters_precede_the_call_candidate_limit() {
    let repo = FixtureRepo::create("java-filtered-calls");
    repo.write("src/Sink.java", "class Sink { static void accept() {} }");
    for index in 0..205 {
        repo.write(
            &format!("src/Caller{index:03}.java"),
            &format!("class Caller{index:03} {{ void run() {{ Sink.accept(); }} }}"),
        );
    }
    repo.write(
        "src/Zulu.java",
        "class Zulu { void finalCaller() { Sink.accept(); } }",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "bounded call filters"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
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
        .expect("index bounded fixture");
    let definitions = query(&service, "accept", CodeQueryKind::Definition).await;
    let id = definitions
        .results
        .iter()
        .filter_map(|hit| hit.canonical_symbol_id.as_deref())
        .find(|id| id.ends_with(".accept"))
        .unwrap();
    for filter in [
        "path:Zulu",
        "name:finalCaller",
        "path:Zulu name:finalCaller",
    ] {
        let hits = query(&service, &format!("{id} {filter}"), CodeQueryKind::Callers).await;
        assert_eq!(hits.results.len(), 1, "{filter}: {:?}", hits.results);
        assert_eq!(hits.results[0].path, "src/Zulu.java");
    }
}

#[tokio::test]
async fn overloaded_java_methods_require_the_definition_snapshot_selector() {
    let repo = FixtureRepo::create("java-overloaded-calls");
    repo.write("src/Worker.java", "class Worker {\n void dispatch() {\n first();\n }\n void dispatch(int value) {\n second();\n }\n void first() {}\n void second() {}\n}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "overloaded methods"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
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
        .expect("index overload fixture");
    let definitions = query(&service, "dispatch", CodeQueryKind::Definition).await;
    let symbols = definitions
        .results
        .iter()
        .filter(|hit| hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol))
        .filter(|hit| {
            hit.canonical_symbol_id
                .as_deref()
                .is_some_and(|id| id.ends_with(".dispatch"))
        })
        .collect::<Vec<_>>();
    assert_eq!(symbols.len(), 2);
    let obsolete_snapshot = symbols[0].symbol_snapshot_id.clone().unwrap();
    let canonical = symbols[0].canonical_symbol_id.as_ref().unwrap();
    let error = service
        .query_code_repository(
            CodeRetrievalRequest::new(
                canonical,
                selector("fixture", "HEAD"),
                CodeQueryKind::Callees,
                10,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("ambiguous"),
        )
        .await
        .expect_err("canonical must not merge overload bodies");
    assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    assert!(error.message.contains("symbol_snapshot_id"));
    let mut called = std::collections::BTreeSet::new();
    for symbol in symbols {
        let id = symbol.symbol_snapshot_id.as_deref().unwrap();
        let hits = query(&service, id, CodeQueryKind::Callees).await;
        assert_eq!(hits.results.len(), 1, "{id}: {:?}", hits.results);
        called.insert(hits.results[0].canonical_symbol_id.clone().unwrap());
    }
    assert!(called.iter().any(|id| id.ends_with(".first")));
    assert!(called.iter().any(|id| id.ends_with(".second")));
    assert!(
        query(&service, "symbol:does-not-exist", CodeQueryKind::Callees)
            .await
            .results
            .is_empty()
    );
    repo.write("src/Extra.java", "class Extra {}\n");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "next snapshot"]);
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("next-index"),
        )
        .await
        .expect("index subsequent snapshot");
    assert!(
        query(&service, &obsolete_snapshot, CodeQueryKind::Callees)
            .await
            .results
            .is_empty()
    );
}

#[tokio::test]
async fn cpp_canonical_call_queries_ignore_prototypes_before_the_unique_definition() {
    let repo = FixtureRepo::create("cpp-canonical-prototypes");
    repo.write("src/main.cpp", "void helper();\nvoid helper();\nvoid leaf() {}\nvoid helper() { leaf(); }\nvoid caller() { helper(); }\n");
    repo.write(
        "src/external.cpp",
        "void external(); void source() { external(); }\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "C++ prototypes and implementation"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "fixture").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-prototypes"),
        )
        .await
        .unwrap();
    let definitions = query(&service, "helper", CodeQueryKind::Definition).await;
    let canonical = definitions
        .results
        .iter()
        .find_map(|hit| {
            hit.canonical_symbol_id
                .as_deref()
                .filter(|id| id.ends_with("::helper"))
        })
        .unwrap();
    let callers = query(&service, canonical, CodeQueryKind::Callers).await;
    assert_eq!(callers.results.len(), 1);
    assert!(
        callers.results[0]
            .canonical_symbol_id
            .as_deref()
            .unwrap()
            .ends_with("::caller")
    );
    let callees = query(&service, canonical, CodeQueryKind::Callees).await;
    assert_eq!(callees.results.len(), 1);
    assert!(
        callees.results[0]
            .canonical_symbol_id
            .as_deref()
            .unwrap()
            .ends_with("::leaf")
    );
    let declarations = query(&service, "external", CodeQueryKind::Definition).await;
    let declaration = declarations
        .results
        .iter()
        .find_map(|hit| {
            hit.canonical_symbol_id
                .as_deref()
                .filter(|id| id.ends_with("::external"))
        })
        .unwrap();
    let callers = query(&service, declaration, CodeQueryKind::Callers).await;
    assert_eq!(callers.results.len(), 1);
    assert!(
        callers.results[0]
            .canonical_symbol_id
            .as_deref()
            .unwrap()
            .ends_with("::source")
    );
}
