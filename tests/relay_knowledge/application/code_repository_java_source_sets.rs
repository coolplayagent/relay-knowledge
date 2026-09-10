use super::*;

#[tokio::test]
async fn conventional_java_source_sets_preserve_main_reads_and_provider_lifecycle() {
    let repo = FixtureRepo::create("java-source-sets");
    repo.write("pom.xml", "<project/>");
    repo.write("src/main/java/p/App.java", "package p; class App { String read() { return System.getProperty(\"main.flag\", \"true\"); } }");
    repo.write("src/test/java/p/System.java", "package p; class System { static String getProperty(String key, String fallback) { return \"custom\"; } }");
    repo.write("other/src/main/java/p/System.java", "package p; class System { static String getProperty(String key, String fallback) { return \"custom\"; } }");
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Independent Java source sets"]);
    let service = service_with_memory_store().await;
    register_complete_java_fixture_repo(&service, &repo, "register-source-sets").await;
    for (stage, expected) in [(0, 1), (1, 0), (2, 1), (3, 1)] {
        if stage == 1 {
            repo.write("src/main/java/p/Misc.java", "package p; class System { static String getProperty(String key, String fallback) { return \"local\"; } }");
            repo.git(["add", "."]);
            repo.git(["commit", "-m", "Add same main provider"]);
        } else if stage == 2 {
            repo.git(["rm", "src/main/java/p/Misc.java"]);
            repo.git(["commit", "-m", "Delete main provider"]);
        }
        if stage == 3 {
            repo.write(
                "src/test/java/p/Invalid.java",
                "package p; class Invalid { broken(",
            );
            repo.git(["add", "."]);
            repo.git(["commit", "-m", "Unrelated incomplete test unit"]);
        }
        service
            .index_code_repository(
                CodeIndexRequest {
                    repository: selector("fixture", "HEAD"),
                    mode: if stage == 0 {
                        CodeIndexMode::Full
                    } else {
                        CodeIndexMode::incremental("HEAD~1", "HEAD").unwrap()
                    },
                    workspace_detection: Default::default(),
                    freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    reuse_historical: false,
                },
                context("index-source-sets"),
            )
            .await
            .unwrap();
        let result = service
            .query_code_repository_feature_flags(
                CodeFeatureFlagRequest::new(
                    None,
                    filtered_selector("fixture", "HEAD", "src/main/java"),
                    50,
                    FreshnessPolicy::WaitUntilFresh,
                )
                .unwrap(),
                context("query-source-sets"),
            )
            .await
            .unwrap();
        if stage != 3 {
            assert!(result.degraded_reason.is_none());
        }
        assert_eq!(
            result.flags.len(),
            expected,
            "stage {stage}: {:?}",
            result.flags
        );
        if expected == 1 {
            assert_eq!(result.flags[0].source_key, "main.flag");
        }
    }
}
