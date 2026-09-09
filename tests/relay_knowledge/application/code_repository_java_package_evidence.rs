//! Same-package type proofs use the current authorized scope, including copied reads.
use super::*;
use relay_knowledge::{application::RelayKnowledgeService, domain::CodeRepositorySelector};

const APP: &str = r#"package p;
class Keys { static final String FLAG="implicit_key"; }
interface Config { String getValue(); }
class DefaultConfig implements Config {
 public String getValue() { return System.getProperty(Keys.FLAG); }
}
class App {
 String direct() { return System.getProperty("implicit_direct"); }
 boolean use(Config config) { if (config.getValue().equals("true")) { return true; } return false; }
 String qualified() { return java.lang.System.getProperty("qualified_control"); }
}
"#;
const PROVIDER: &str =
    "package p; class System { static String getProperty(String name) { return name; } }";

#[tokio::test]
async fn java_package_comments_preserve_reads_and_same_package_shadowing() {
    let repo = FixtureRepo::create("java-package-comments");
    repo.write("src/Main.java", "package sample /* legal package comment */ . nested; class Main { String read() { return System.getProperty(\"feature_x\"); } }");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Legal package comment"]);
    let service = service_with_memory_store().await;
    register_complete_java_fixture_repo(&service, &repo, "register-comment-package").await;
    for provider in [false, true] {
        if provider {
            repo.write("arbitrary/Misc.java", "package sample.nested; class System { static String getProperty(String key) { return key; } }");
            repo.git(["add", "."]);
            repo.git(["commit", "-m", "Actual same package"]);
        }
        index(&service, CodeIndexMode::Full).await;
        let result = service
            .query_code_repository_feature_flags(
                CodeFeatureFlagRequest::new(
                    None,
                    selector("fixture", "HEAD"),
                    50,
                    FreshnessPolicy::WaitUntilFresh,
                )
                .unwrap(),
                context("query-comment-package"),
            )
            .await
            .unwrap();
        assert!(result.degraded_reason.is_none());
        assert_eq!(
            result.flags.len(),
            usize::from(!provider),
            "{:?}",
            result.flags
        );
        if !provider {
            assert_eq!(result.flags[0].source_key, "feature_x");
        }
    }
}

#[tokio::test]
async fn java_same_package_providers_reclassify_copied_reads_and_preserve_history() {
    for overlay in [false, true] {
        let repo = FixtureRepo::create("java-package-lifecycle");
        repo.write("src/App.java", APP);
        repo.write("src/Explicit.java", "package p; import java.lang.System; class Explicit { String read() { return System.getProperty(\"explicit_control\"); } }");
        repo.git(["add", "."]);
        repo.git(["commit", "-m", "Platform reads"]);
        let base = repo.git_text(["rev-parse", "HEAD"]);
        let service = service_with_memory_store().await;
        register_complete_java_fixture_repo(&service, &repo, "register-package-proof").await;
        index(&service, CodeIndexMode::Full).await;
        assert_keys(&service, "HEAD", true).await;
        // Directory and filename deliberately do not resemble the declared package/type.
        repo.write("elsewhere/Misc.java", PROVIDER);
        let (reference, added) = if overlay {
            index(&service, CodeIndexMode::WorktreeOverlay).await;
            ("worktree", None)
        } else {
            repo.git(["add", "."]);
            repo.git(["commit", "-m", "Same package provider"]);
            let added = repo.git_text(["rev-parse", "HEAD"]);
            index(
                &service,
                CodeIndexMode::Incremental {
                    base_ref: base.clone(),
                    head_ref: added.clone(),
                },
            )
            .await;
            ("HEAD", Some(added))
        };
        assert_keys(&service, reference, false).await;
        assert_keys(&service, &base, true).await;
        std::fs::remove_file(repo.path.join("elsewhere/Misc.java")).unwrap();
        if let Some(added) = added {
            repo.git(["add", "-A"]);
            repo.git(["commit", "-m", "Remove provider"]);
            let head = repo.git_text(["rev-parse", "HEAD"]);
            index(
                &service,
                CodeIndexMode::Incremental {
                    base_ref: added.clone(),
                    head_ref: head,
                },
            )
            .await;
            assert_keys(&service, &added, false).await;
        } else {
            index(&service, CodeIndexMode::WorktreeOverlay).await;
        }
        assert_keys(&service, reference, true).await;
    }
}

#[tokio::test]
async fn java_restricted_authorization_does_not_prove_package_absence() {
    let repo = FixtureRepo::create("java-package-restricted");
    repo.write("src/App.java", APP);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Restricted Java scope"]);
    let service = service_with_memory_store().await;
    register_fixture_repo(&service, &repo, "register-restricted-java").await;
    index(&service, CodeIndexMode::Full).await;
    let result = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(
                None,
                selector("fixture", "HEAD"),
                50,
                FreshnessPolicy::WaitUntilFresh,
            )
            .unwrap(),
            context("query-restricted-java"),
        )
        .await
        .unwrap();
    assert!(result.degraded_reason.is_none());
    assert_eq!(
        result
            .flags
            .iter()
            .map(|flag| flag.source_key.as_str())
            .collect::<Vec<_>>(),
        ["qualified_control"]
    );
}

async fn index(service: &RelayKnowledgeService, mode: CodeIndexMode) {
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-java-package"),
        )
        .await
        .unwrap();
}

async fn assert_keys(service: &RelayKnowledgeService, reference: &str, implicit: bool) {
    // A query filter may narrow returned reads, but must not hide the provider inventory.
    let request_scope =
        CodeRepositorySelector::new("fixture", reference, vec!["src".to_owned()], Vec::new())
            .unwrap();
    let result = service
        .query_code_repository_feature_flags(
            CodeFeatureFlagRequest::new(None, request_scope, 50, FreshnessPolicy::WaitUntilFresh)
                .unwrap(),
            context("query-java-package"),
        )
        .await
        .unwrap();
    assert!(result.degraded_reason.is_none());
    for name in ["qualified_control", "explicit_control"] {
        assert!(
            result.flags.iter().any(|flag| flag.source_key == name),
            "{:?}",
            result.flags
        );
    }
    for name in ["implicit_key", "implicit_direct"] {
        assert_eq!(
            result.flags.iter().any(|flag| flag.source_key == name),
            implicit,
            "{:?}",
            result.flags
        );
    }
    if implicit {
        let flag = result
            .flags
            .iter()
            .find(|flag| flag.source_key == "implicit_key")
            .unwrap();
        assert!(
            flag.usages
                .iter()
                .any(|usage| usage.edge_kind == "declares_config_key")
        );
        assert!(
            flag.usages
                .iter()
                .any(|usage| usage.edge_kind == "guards_code")
        );
    }
}
