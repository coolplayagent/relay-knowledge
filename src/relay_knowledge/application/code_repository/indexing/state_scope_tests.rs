use super::*;
use crate::{
    api::CodeRepositoryRegisterRequest,
    application::code_repository::repository::test_support::{
        FixtureRepo, context, indexed_language_fixture, service_with_memory_store,
    },
    domain::{CodeIndexTaskState, CodeRepositoryRegistration, FreshnessPolicy},
    storage::KnowledgeStore,
};
use std::sync::Arc;

#[tokio::test]
async fn historical_reuse_preserves_public_languages_in_durable_payload() {
    let repo = FixtureRepo::create("historical-language-groups");
    repo.write("src/pom.xml", "<project><dependencies><dependency><groupId>test</groupId><artifactId>before</artifactId><version>1</version></dependency></dependencies></project>");
    repo.write("src/Owner.java", "class Owner {}");
    repo.write("src/Owner.kt", "class Owner");
    repo.git(["add", "src"]);
    repo.git(["commit", "-m", "base shared manifest"]);
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".into(),
                path_filters: vec!["src".into()],
                language_filters: vec![],
            },
            context("register-shared-manifest"),
        )
        .await
        .unwrap();
    // Older persisted registrations may still carry language restrictions even
    // though new CLI registrations expose the full language surface.
    let store = service.store().await.unwrap();
    let status = store
        .code_repository_status("fixture".into())
        .await
        .unwrap()
        .unwrap();
    store
        .upsert_code_repository(
            CodeRepositoryRegistration::new(
                status.repository_id,
                status.alias,
                status.root_path,
                status.path_filters,
                vec!["java".into()],
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let mut request = CodeIndexRequest {
        repository: CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["kotlin".into()])
            .unwrap(),
        mode: CodeIndexMode::Full,
        workspace_detection: Default::default(),
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        reuse_historical: false,
    };
    let initial = service
        .index_code_repository(request.clone(), context("index-shared-base"))
        .await
        .unwrap();
    assert_eq!(initial.summary.indexed_file_count, 1);
    repo.write("src/pom.xml", "<project><dependencies><dependency><groupId>test</groupId><artifactId>after</artifactId><version>2</version></dependency></dependencies></project>");
    repo.git(["add", "src"]);
    repo.git(["commit", "-m", "change shared manifest"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);
    request.reuse_historical = true;
    let started = service
        .start_code_repository_index(request.clone(), context("reuse-shared-manifest"))
        .await
        .unwrap();
    let task = started.task.unwrap();
    assert_eq!(
        task.mode,
        CodeIndexMode::incremental(base, head.clone()).unwrap()
    );
    assert_eq!(
        task.language_filters,
        crate::domain::code_scope_language_filters(&["java".into()], &["kotlin".into()])
    );
    let payload: CodeIndexRequest = serde_json::from_str(&task.payload_json).unwrap();
    assert_eq!(payload.repository.language_filters, ["kotlin"]);
    let duplicate = service
        .start_code_repository_index(request, context("reuse-shared-duplicate"))
        .await
        .unwrap();
    assert_eq!(duplicate.task.unwrap().task_id, task.task_id);
    let completed = service
        .run_code_index_task_once(Some(task.task_id), context("run-shared-task"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.state, CodeIndexTaskState::Succeeded);
    assert_eq!(completed.resolved_commit_sha, head);
    let files = store
        .code_file_fingerprints_for_scope(completed.source_scope.clone())
        .await
        .unwrap();
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["src/pom.xml"]
    );
    let software = service
        .software_global_projection(
            crate::domain::SoftwareGlobalRequest::new(
                CodeRepositorySelector::new("fixture", "HEAD", vec![], vec!["kotlin".into()])
                    .unwrap(),
                crate::domain::SoftwareGlobalKind::All,
                FreshnessPolicy::WaitUntilFresh,
                10,
            )
            .unwrap(),
            context("historical-reuse-sbom"),
        )
        .await
        .unwrap();
    assert!(
        software
            .components
            .iter()
            .any(|component| component.name == "test:after"),
        "{:?}",
        software.components
    );
    assert!(
        software
            .components
            .iter()
            .all(|component| component.name != "test:before")
    );
}

#[tokio::test]
async fn previous_state_rejects_cross_language_base_for_incremental_and_overlay() {
    let (repo, _, sqlite) = indexed_language_fixture(&["java"]).await;
    let base = repo.git_text(["rev-parse", "HEAD"]);
    let store: Arc<dyn KnowledgeStore> = sqlite;
    let status = store
        .code_repository_status("fixture".into())
        .await
        .unwrap()
        .unwrap();
    for mode in [
        CodeIndexMode::incremental(base.clone(), base.clone()).unwrap(),
        CodeIndexMode::WorktreeOverlay,
    ] {
        let mut request = CodeIndexRequest {
            repository: CodeRepositorySelector::new(
                "fixture",
                base.clone(),
                vec![],
                vec!["java".into()],
            )
            .unwrap(),
            mode,
            workspace_detection: Default::default(),
            freshness_policy: FreshnessPolicy::WaitUntilFresh,
            reuse_historical: false,
        };
        let previous = previous_index_state_for_index(&store, &status, &request)
            .await
            .unwrap();
        assert_eq!(
            previous.base_resolved_commit_sha.as_deref(),
            Some(base.as_str())
        );
        assert_eq!(previous.fingerprints.len(), 1);
        request.repository.language_filters = vec!["python".into()];
        let error = previous_index_state_for_index(&store, &status, &request)
            .await
            .err()
            .expect("missing language must reject reuse");
        assert!(error.message.contains("base scope"), "{error:?}");
    }
}
