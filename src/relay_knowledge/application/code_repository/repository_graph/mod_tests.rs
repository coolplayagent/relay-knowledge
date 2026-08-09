use crate::{
    api::CodeRepositoryRegisterRequest,
    domain::{
        CodeIndexMode, CodeIndexRequest, CodeRepositorySelector, FreshnessPolicy,
        RepositoryGraphNeighborhoodRequest,
    },
};

use super::super::repository::test_support::{FixtureRepo, context, service_with_memory_store};

#[tokio::test]
async fn repository_graph_neighborhood_is_bound_to_the_indexed_okf_snapshot() {
    let repo = FixtureRepo::create("okf-neighborhood");
    repo.write(
        "knowledge/investment-research/rates.md",
        "---\ntype: Research Claim\ntitle: 利率\nsources:\n  - id: pbc\n    resource: https://www.pbc.gov.cn/rates\n---\n\n政策利率。[^pbc]\n\n[^pbc]: 人民银行\n",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "add knowledge"]);
    let head = repo.git_text(["rev-parse", "HEAD"]);
    let service = service_with_memory_store().await;
    service
        .register_code_repository(
            CodeRepositoryRegisterRequest {
                root_path: repo.path.display().to_string(),
                alias: "fixture".to_owned(),
                path_filters: vec!["knowledge/investment-research".to_owned()],
                language_filters: Vec::new(),
            },
            context("register-okf-neighborhood"),
        )
        .await
        .expect("repository should register");
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: CodeRepositorySelector::new("fixture", "HEAD", Vec::new(), Vec::new())
                    .expect("selector"),
                mode: CodeIndexMode::Full,
                workspace_detection: Default::default(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
            },
            context("index-okf-neighborhood"),
        )
        .await
        .expect("repository should index");

    let response = service
        .repository_graph_neighborhood(
            RepositoryGraphNeighborhoodRequest::new(
                CodeRepositorySelector::new(
                    "fixture",
                    head.clone(),
                    vec!["knowledge/investment-research".to_owned()],
                    vec!["markdown".to_owned()],
                )
                .expect("selector"),
                "knowledge/investment-research/rates.md",
                1,
                100,
                200,
            )
            .expect("request"),
            context("query-okf-neighborhood"),
        )
        .await
        .expect("neighborhood should query");

    assert_eq!(response.schema_version, 1);
    assert_eq!(response.scope.resolved_commit_sha, head);
    assert!(!response.scope.stale);
    assert!(response.nodes.iter().any(|node| node.kind == "okf_concept"));
    assert!(
        response
            .nodes
            .iter()
            .any(|node| node.kind == "external_source")
    );
    assert!(
        response
            .edges
            .iter()
            .any(|edge| edge.kind == "cites_source")
    );
}
