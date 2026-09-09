//! Actual Git indexing preserves Java receiver proofs and inherited key ownership.
use super::*;
#[tokio::test]
async fn java_nested_parent_fields_constants_and_getter_contracts_survive_real_git_indexing() {
    let repo = FixtureRepo::create("java-parent-contracts");
    repo.write(
        "src/Main.java",
        r#"
class Endpoint { String getProperty(String key) { return key; } }
class Lang { Endpoint System = new Endpoint(); }
class Chain { Lang lang = new Lang(); }
class Parent { protected Chain java = new Chain(); }
class Shadow extends Parent {
 String read() { return java.lang.System.getProperty("NOT_CONFIG"); }
}
class Outer { static class Base { protected static final String FLAG = "nested.key"; } }
class Nested extends Outer.Base {
 String read() { return System.getProperty(FLAG, "nested-default"); }
}
abstract class BaseConfig { abstract String getValue(); }
class DefaultConfig extends BaseConfig {
 @Override String getValue() { return System.getProperty("feature_x", "true"); }
}
class Main {
 boolean consume(BaseConfig config) {
  if (config.getValue().equals("true")) { return true; }
  return false;
 }
}
"#,
    );
    repo.write(
        "src/flags.properties",
        "nested.key=nested-default\nfeature_x=true\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Add parent contracts"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-parent-contracts").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-parent-contracts"),
        )
        .await
        .unwrap();
    let result = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                50,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("query-parent-contracts"),
        )
        .await
        .unwrap();
    assert_eq!(result.flags.len(), 2, "{:?}", result.flags);
    let nested = result
        .flags
        .iter()
        .find(|f| f.source_key == "nested.key")
        .unwrap();
    assert_eq!(
        nested
            .usages
            .iter()
            .filter(|u| u.edge_kind == "reads_config")
            .count(),
        1
    );
    assert_eq!(
        nested
            .usages
            .iter()
            .filter(|u| u.edge_kind == "declares_config_key")
            .count(),
        1
    );
    let flag = result
        .flags
        .iter()
        .find(|f| f.source_key == "feature_x")
        .unwrap();
    assert_eq!(
        flag.usages
            .iter()
            .filter(|u| u.edge_kind == "reads_config")
            .count(),
        2
    );
    assert_eq!(
        flag.usages
            .iter()
            .filter(|u| u.edge_kind == "guards_code")
            .count(),
        1
    );
    assert!(flag.usages.iter().any(|u| {
        u.metadata
            .bindings
            .iter()
            .any(|b| b == "BaseConfig.getValue")
    }));
}
#[tokio::test]
async fn java_qualified_shadows_inherited_keys_and_text_fallback_keep_proof_boundaries() {
    let repo = FixtureRepo::create("java-qualified-inherited-fallback");
    repo.write(
        "src/App.java",
        r#"class Chain { Lang lang=new Lang(); } class Lang { Api System=new Api(); } class Api { String getProperty(String key){ return key; } }
 interface Keys { String FLAG="inherited.key"; }
 interface Middle extends Keys {}
 class App implements Middle {
 void run(Chain java) {
  java.lang.System.getProperty("FALSE_QUALIFIED");
  System.getProperty(FLAG, "true");
 }
 void local(String FLAG) { System.getProperty(FLAG); }
 void real() { java.lang.System.getProperty("REAL_QUALIFIED"); }
 }"#,
    );
    repo.write("src/app.properties", "inherited.key=true\n");
    let read = "class Large { void run() { if(System.getenv(\"FALSE_ENV\")!=null) {} } }\n";
    repo.write(
        "src/Large.java",
        &format!("{read}/*{}*/", "x".repeat(512 * 1024 + 1)),
    );
    let mut invalid = read.as_bytes().to_vec();
    invalid.extend_from_slice(b"//\xff\n");
    std::fs::write(repo.path.join("src/Invalid.java"), invalid).unwrap();
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Java inherited and fallback proofs"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-java-proof").await;
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-java-proof"),
        )
        .await
        .unwrap();
    let result = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                50,
                FreshnessPolicy::AllowStale,
            )
            .unwrap(),
            context("query-java-proof"),
        )
        .await
        .unwrap();
    assert_eq!(result.flags.len(), 2, "{:?}", result.flags);
    let inherited = result
        .flags
        .iter()
        .find(|f| f.source_key == "inherited.key")
        .unwrap();
    assert_eq!(
        inherited
            .usages
            .iter()
            .filter(|u| u.edge_kind == "reads_config")
            .count(),
        1
    );
    assert_eq!(
        inherited
            .usages
            .iter()
            .filter(|u| u.edge_kind == "declares_config_key")
            .count(),
        1
    );
    assert!(
        result
            .flags
            .iter()
            .any(|f| f.source_key == "REAL_QUALIFIED")
    );
}
