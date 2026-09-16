//! Issue #388: class-name calls must aggregate member edges in both directions.
use super::*;

#[tokio::test]
async fn java_class_call_queries_return_member_edges_without_reverse_text_matches() {
    let repo = FixtureRepo::create("java-class-calls");
    repo.write(
        "src/misc/A.java",
        "package demo; public class A { public static void main(String[] args) { B.process(); } }",
    );
    // Package declarations deliberately do not mirror the repository directory.
    repo.write("src/misc/B.java", "package demo; public class B { public static void process() { System.out.println(\"processed\"); } }");
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

#[tokio::test]
async fn persisted_type_calls_cover_supported_type_languages_with_same_named_members() {
    let repo = FixtureRepo::create("portable-type-calls");
    let cases = [
        (
            "Owner.java",
            "java",
            "class Owner { void run() { target(); } void target() {} }",
        ),
        (
            "owner.py",
            "python",
            "class Owner:\n    def run(self):\n        self.target()\n    def target(self):\n        pass\n",
        ),
        (
            "owner.js",
            "javascript",
            "class Owner { run() { this.target(); } target() {} }",
        ),
        (
            "owner.jsx",
            "jsx",
            "class Owner { run() { this.target(); } target() {} }",
        ),
        (
            "owner.ts",
            "typescript",
            "class Owner { run() { this.target(); } target() {} }",
        ),
        (
            "owner.tsx",
            "tsx",
            "class Owner { run() { this.target(); } target() {} }",
        ),
        (
            "owner.cpp",
            "cpp",
            "struct V {}; template<class T> class Owner { public: template<class U> void run(); void target() {} }; template<> class Owner<V> {public: void extra() { special_target(); }}; template<class V> template<class U> void Owner<V>::run() { target(); }",
        ),
        (
            "Owner.cs",
            "csharp",
            "class Owner { void run() { target(); } void target() {} }",
        ),
        (
            "owner.rs",
            "rust",
            "struct Owner; impl Owner { fn run(&self) { self.target(); } fn target(&self) {} }",
        ),
        (
            "owner.go",
            "go",
            "package demo\ntype Owner struct {}\nfunc (o Owner) run() { o.target() }\nfunc (o Owner) target() {}",
        ),
        (
            "Owner.kt",
            "kotlin",
            "class Owner { fun run() { target() }; fun target() {} }",
        ),
        (
            "Owner.scala",
            "scala",
            "class Owner { def run(): Unit = { target() }; def target(): Unit = {} }",
        ),
        (
            "owner.rb",
            "ruby",
            "class Owner\n def run\n  target()\n end\n def target\n end\nend\n",
        ),
        (
            "owner.php",
            "php",
            "<?php class Owner { function run() { $this->target(); } function target() {} }",
        ),
        (
            "owner.swift",
            "swift",
            "class Owner {\n func run() { target() }\n func target() {}\n}",
        ),
        (
            "owner-js.vue",
            "vue",
            "<script>class Owner { run() { this.target(); } target() {} }</script><template><div /></template>",
        ),
        (
            "owner-ts.vue",
            "vue",
            "<script lang=\"ts\">class Owner { run(): void { this.target(); } target(): void {} }</script><template><div /></template>",
        ),
    ];
    for (path, _, source) in cases {
        repo.write(&format!("src/{path}"), source);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "portable types"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-portable-types").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-portable-types"),
        )
        .await
        .unwrap();
    let mut failures = Vec::new();
    for (path, language, _) in cases {
        let mut request = CodeRetrievalRequest::new(
            "Owner",
            selector("fixture", "HEAD"),
            CodeQueryKind::Callees,
            20,
            FreshnessPolicy::AllowStale,
        )
        .unwrap();
        request.query_language_filters = vec![language.to_owned()];
        request.query_path_substrings = vec![path.to_owned()];
        let result = service
            .query_code_repository(request, context("query-portable-types"))
            .await
            .unwrap();
        if !result
            .results
            .iter()
            .any(|r| r.excerpt.contains("run calls target"))
        {
            failures.push(format!("{language}: {:?}", result.results));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
