//! Concurrent cold indexing must retain properties facts across scanner workers.
use super::*;
use relay_knowledge::domain::CodeFeatureFlagRequest;

#[tokio::test]
async fn repeated_mixed_configuration_cold_indexes_keep_all_properties_files() {
    let repo = FixtureRepo::create("properties-worker-isolation");
    for shard in 0..12 {
        repo.write(
            &format!("config/noise_{shard:02}.properties"),
            &(0..1000)
                .map(|key| format!("unrelated_{shard:02}_{key:04}=true\n"))
                .collect::<String>(),
        );
        repo.write(
            &format!("config/metadata_{shard:02}.properties"),
            &(0..100)
                .map(|key| format!("# @config domain=selected hot-reload=true\nmetadata_{shard:02}_{key:03}=true\n"))
                .collect::<String>(),
        );
        repo.write(
            &format!("config/settings_{shard:02}.yaml"),
            &format!("yaml_{shard:02}: true\n"),
        );
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "Concurrent properties scanner regression"]);
    for round in 0..3 {
        let service = service_with_memory_store().await;
        service
            .register_code_repository(
                CodeRepositoryRegisterRequest {
                    root_path: repo.path.display().to_string(),
                    alias: "fixture".to_owned(),
                    path_filters: Vec::new(),
                    language_filters: Vec::new(),
                },
                context("register-properties-workers"),
            )
            .await
            .unwrap();
        let indexed = service
            .index_code_repository(
                CodeIndexRequest {
                    repository: selector("fixture", "HEAD"),
                    mode: CodeIndexMode::Full,
                    workspace_detection: Default::default(),
                    freshness_policy: FreshnessPolicy::WaitUntilFresh,
                    reuse_historical: false,
                },
                context("index-properties-workers"),
            )
            .await
            .unwrap();
        assert_eq!(indexed.summary.indexed_file_count, 36, "cold round {round}");
        assert_eq!(indexed.summary.degraded_file_count, 0, "cold round {round}");
        assert_eq!(indexed.summary.progress.parsed_file_count, 36);
        assert_eq!(indexed.summary.progress.skipped_file_count, 0);
        let flags = service
            .query_code_repository_feature_flags(
                CodeFeatureFlagRequest::new(
                    None,
                    CodeRepositorySelector::new(
                        "fixture",
                        "HEAD",
                        vec!["config/metadata_00.properties".into()],
                        vec![],
                    )
                    .unwrap(),
                    100,
                    FreshnessPolicy::WaitUntilFresh,
                )
                .unwrap(),
                context("query-properties-workers"),
            )
            .await
            .unwrap();
        assert!(flags.degraded_reason.is_none(), "cold round {round}");
        assert_eq!(flags.flags.len(), 100, "cold round {round}");
    }
}
