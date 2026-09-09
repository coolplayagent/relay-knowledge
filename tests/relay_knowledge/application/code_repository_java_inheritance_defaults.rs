//! Actual Git indexing keeps Java inheritance and runtime default values consistent.
use super::*;

#[tokio::test]
async fn java_static_import_inheritance_and_escaped_defaults_survive_indexing() {
    let repo = FixtureRepo::create("java-inheritance-defaults");
    for (path, source) in [
        (
            "custom/Object.java",
            "package custom; class Object { protected static String getenv(String key) { return key; } }",
        ),
        (
            "custom/App.java",
            "package custom; import static java.lang.System.getenv; class App extends Object { void run() { getenv(\"OBJECT_FALSE\"); } }",
        ),
        (
            "custom/Qualified.java",
            "package custom; import static java.lang.System.getenv; class Qualified extends java.lang.Object { void run() { getenv(\"QUALIFIED_REAL\"); } }",
        ),
        (
            "custom/Explicit.java",
            "package custom; import java.lang.Object; import static java.lang.System.getenv; class Explicit extends Object { void run() { getenv(\"EXPLICIT_REAL\"); } }",
        ),
        (
            "privatecase/App.java",
            "package privatecase; import static java.lang.System.getenv; class Base { private static String getenv(String key) { return key; } } class App extends Base { void run() { getenv(\"PRIVATE_REAL\"); } }",
        ),
        (
            "interfacecase/App.java",
            "package interfacecase; import static java.lang.System.getenv; interface Base { static String getenv(String key) { return key; } } class App implements Base { void run() { getenv(\"INTERFACE_REAL\"); } }",
        ),
        (
            "inherited/App.java",
            "package inherited; import static java.lang.System.getenv; class Base { protected static String getenv(String key) { return key; } } class App extends Base { void run() { getenv(\"INHERITED_FALSE\"); } }",
        ),
        (
            "defaults/App.java",
            r#"package defaults; class App {
 static final String KEY="\146lag";
 static final String UNUSED="flag";
 void run() {
 System.getProperty("\u0066lag", "true");
 System.getProperty(KEY, "true");
 System.getProperty("\\u0066lag", "distinct");
 System.getProperty("unicode_flag", "\u0074rue");
 System.getProperty("octal_flag", "\164rue");
 System.getProperty("escaped_backslash", "\\u0074rue");
} }"#,
        ),
        (
            "defaults/app.properties",
            "unicode_flag=true\noctal_flag=true\nflag=true\n",
        ),
    ] {
        repo.write(&format!("src/{path}"), source);
    }
    repo.git(["add", "."]);
    repo.git([
        "commit",
        "-m",
        "Java inheritance and escaped default values",
    ]);
    let service = service_with_memory_store().await;
    register_complete_java_fixture_repo(&service, &repo, "register-inheritance-defaults").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-inheritance-defaults"),
        )
        .await
        .unwrap();
    let mut request = CodeFeatureFlagRequest::new(
        None,
        selector("fixture", "HEAD"),
        50,
        FreshnessPolicy::WaitUntilFresh,
    )
    .unwrap();
    request.consistency = true;
    let response = service
        .query_code_repository_feature_flags(request, context("query-inheritance-defaults"))
        .await
        .unwrap();
    assert_eq!(response.flags.len(), 9, "{:?}", response.flags);
    for key in [
        "QUALIFIED_REAL",
        "EXPLICIT_REAL",
        "PRIVATE_REAL",
        "INTERFACE_REAL",
    ] {
        let flag = response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap();
        assert_eq!(flag.source_kind, "env_var");
        assert_eq!(
            flag.usages
                .iter()
                .filter(|usage| usage.edge_kind == "reads_config")
                .count(),
            1
        );
    }
    for key in ["unicode_flag", "octal_flag"] {
        let flag = response
            .flags
            .iter()
            .find(|flag| flag.source_key == key)
            .unwrap();
        assert!(
            flag.consistency_diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.starts_with("conflicting_defaults:")),
            "{flag:?}"
        );
        assert_eq!(
            flag.usages
                .iter()
                .find(|usage| usage.edge_kind == "reads_config")
                .unwrap()
                .metadata
                .default_value
                .as_deref(),
            Some("true")
        );
    }
    let literal = response
        .flags
        .iter()
        .find(|flag| flag.source_key == "escaped_backslash")
        .unwrap();
    assert_eq!(
        literal.usages[0].metadata.default_value.as_deref(),
        Some(r"\u0074rue")
    );
    let flag = response
        .flags
        .iter()
        .find(|f| f.source_key == "flag")
        .unwrap();
    assert_eq!(
        flag.usages
            .iter()
            .filter(|u| u.edge_kind == "reads_config")
            .count(),
        2
    );
    let declarations: Vec<_> = flag
        .usages
        .iter()
        .filter(|u| u.edge_kind == "declares_config_key")
        .collect();
    assert_eq!(declarations.len(), 1);
    assert!(
        declarations[0]
            .metadata
            .bindings
            .iter()
            .any(|b| b.ends_with("App.KEY"))
    );
    assert!(!flag.usages.iter().any(|u| {
        u.metadata
            .bindings
            .iter()
            .any(|b| b.ends_with("App.UNUSED"))
    }));
    assert!(response.flags.iter().any(|f| f.source_key == r"\u0066lag"));
}
