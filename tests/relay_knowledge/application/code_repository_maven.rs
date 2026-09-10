//! Maven reactor behavior through the shared CLI/Web application service.
use super::*;
use relay_knowledge::domain::{CodeImpactRequest, CodeWorkspaceDetectionConfig};

#[path = "code_repository_maven_cli.rs"]
mod cli;

#[tokio::test]
async fn maven_modules_propagate_downstream_and_incremental_removes_old_edges() {
    let repo = FixtureRepo::create("maven-reactor");
    repo.write("pom.xml", "<project><groupId>demo</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><modules><module>a</module><module>b</module><module>c</module></modules></project>");
    for (name, dependency) in [("a", Some("b")), ("b", Some("c")), ("c", None)] {
        write_module(&repo, name, dependency);
    }
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "reactor"]);
    let first = repo.git_text(["rev-parse", "HEAD"]);
    let service = maven_service(&repo).await;
    register_fixture_repo(&service, &repo, vec![], "register-maven").await;
    let indexed = index(&service, CodeIndexMode::Full).await;
    assert_eq!(indexed.summary.indexed_file_count, 7);
    let projection = modules(&service).await;
    assert_eq!(projection.build_targets.len(), 4);
    assert_eq!(projection.relationships.len(), 5);
    assert!(
        projection
            .relationships
            .iter()
            .all(|edge| edge.resolution_state == "resolved")
    );

    repo.write(
        "c/src/main/java/demo/Api.java",
        "package demo; public class Api { public int value() { return 2; } }",
    );
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "change leaf implementation"]);
    index(
        &service,
        CodeIndexMode::Incremental {
            base_ref: "HEAD~1".into(),
            head_ref: "HEAD".into(),
        },
    )
    .await;
    let impact = service
        .impact_code_repository(
            CodeImpactRequest::new(selector("fixture", "HEAD"), &first, "HEAD", 100).unwrap(),
            context("impact-maven"),
        )
        .await
        .unwrap();
    let chains = impact
        .results
        .iter()
        .filter(|hit| hit.edge_kind.as_deref() == Some("module_depends_on"))
        .collect::<Vec<_>>();
    assert_eq!(chains.len(), 2, "{:#?}", impact.results);
    assert!(
        chains
            .iter()
            .any(|hit| hit.edge_target_hint.as_deref()
                == Some("a/pom.xml -> b/pom.xml -> c/pom.xml"))
    );

    write_module(&repo, "a", None);
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "remove a dependency"]);
    index(
        &service,
        CodeIndexMode::Incremental {
            base_ref: "HEAD~1".into(),
            head_ref: "HEAD".into(),
        },
    )
    .await;
    let updated = modules(&service).await;
    assert_eq!(
        updated
            .relationships
            .iter()
            .filter(|edge| edge.relationship_kind == "depends_on")
            .count(),
        1
    );
    assert_eq!(
        updated
            .build_targets
            .iter()
            .find(|node| node.evidence_path == "a/pom.xml")
            .unwrap()
            .target_id,
        projection
            .build_targets
            .iter()
            .find(|node| node.evidence_path == "a/pom.xml")
            .unwrap()
            .target_id
    );
}

#[tokio::test]
async fn maven_152_modules_preserve_all_direct_dependency_edges() {
    let repo = FixtureRepo::create("maven-152");
    let mut members = String::new();
    for index in 0..152 {
        let name = format!("unit-{index}");
        members.push_str(&format!("<module>{name}</module>"));
        let dependency = (index > 0).then(|| "unit-0".to_owned());
        write_module(&repo, &name, dependency.as_deref());
        let pom_path = format!("{name}/pom.xml");
        let pom = std::fs::read_to_string(repo.path.join(&pom_path)).unwrap();
        let external = "<dependency><groupId>external</groupId><artifactId>logging</artifactId><version>1</version></dependency>";
        let pom = if pom.contains("</dependencies>") {
            pom.replace("</dependencies>", &format!("{external}</dependencies>"))
        } else {
            pom.replace(
                "</project>",
                &format!("<dependencies>{external}</dependencies></project>"),
            )
        };
        repo.write(&pom_path, &pom);
    }
    repo.write("pom.xml", &format!("<project><groupId>demo</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><modules>{members}</modules></project>"));
    repo.git(["add", "."]);
    repo.git(["commit", "-m", "large reactor"]);
    let service = maven_service(&repo).await;
    register_fixture_repo(&service, &repo, vec![], "register-large-maven").await;
    let indexed = index(&service, CodeIndexMode::Full).await;
    assert_eq!(indexed.summary.indexed_file_count, 305);
    assert_eq!(indexed.summary.degraded_file_count, 0);
    for kind in [
        SoftwareGlobalKind::Modules,
        SoftwareGlobalKind::Dependencies,
    ] {
        let mut request = SoftwareGlobalRequest::new(
            selector("fixture", "HEAD"),
            kind,
            FreshnessPolicy::WaitUntilFresh,
            500,
        )
        .unwrap();
        let mut nodes = std::collections::BTreeSet::new();
        let mut edges = std::collections::BTreeSet::new();
        let mut artifacts = 0;
        let mut pages = 0;
        loop {
            let graph = service
                .software_global_projection(request.clone(), context("paged-large-reactor"))
                .await
                .unwrap();
            assert!(
                graph.build_targets.len()
                    + graph.relationships.len()
                    + graph.components.len()
                    + graph.dependency_usages.len()
                    <= 500
            );
            for node in graph.build_targets {
                assert!(nodes.insert(node.target_id));
            }
            for edge in graph.relationships {
                assert!(edges.insert(edge.relationship_id));
                if edge.target_kind == "artifact" {
                    artifacts += 1;
                    assert_eq!(edge.resolution_state, "unresolved");
                } else {
                    assert_eq!(edge.resolution_state, "resolved");
                }
            }
            pages += 1;
            assert!(pages < 20);
            let Some(cursor) = graph.next_cursor else {
                break;
            };
            request.cursor = Some(cursor);
        }
        assert!(pages >= 2);
        assert_eq!(nodes.len(), 153);
        assert_eq!(edges.len(), 455);
        assert_eq!(artifacts, 152);
    }
}

fn write_module(repo: &FixtureRepo, name: &str, dependency: Option<&str>) {
    let dependency = dependency.map(|name| format!("<dependencies><dependency><groupId>demo</groupId><artifactId>{name}</artifactId><version>1</version></dependency></dependencies>")).unwrap_or_default();
    repo.write(&format!("{name}/pom.xml"), &format!("<project><parent><groupId>demo</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>{name}</artifactId>{dependency}</project>"));
    repo.write(
        &format!("{name}/src/main/java/demo/Api.java"),
        "package demo; public class Api { public int value() { return 1; } }",
    );
}

async fn index(
    service: &RelayKnowledgeService,
    mode: CodeIndexMode,
) -> relay_knowledge::api::CodeRepositoryIndexResponse {
    service
        .index_code_repository(
            CodeIndexRequest {
                repository: selector("fixture", "HEAD"),
                mode,
                workspace_detection: CodeWorkspaceDetectionConfig::disabled(),
                freshness_policy: FreshnessPolicy::WaitUntilFresh,
                reuse_historical: false,
            },
            context("index-maven"),
        )
        .await
        .unwrap()
}

async fn modules(service: &RelayKnowledgeService) -> relay_knowledge::api::SoftwareGlobalResponse {
    service
        .software_global_projection(
            SoftwareGlobalRequest::new(
                selector("fixture", "HEAD"),
                SoftwareGlobalKind::Modules,
                FreshnessPolicy::WaitUntilFresh,
                500,
            )
            .unwrap(),
            context("modules-maven"),
        )
        .await
        .unwrap()
}

async fn maven_service(repo: &FixtureRepo) -> RelayKnowledgeService {
    let runtime_root = repo.path.with_extension("runtime").display().to_string();
    let platform = if cfg!(windows) {
        PlatformKind::Windows
    } else {
        PlatformKind::Unix
    };
    let values = [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMPDIR",
        "RELAY_KNOWLEDGE_HOME",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), runtime_root.clone()))
    .collect::<Vec<_>>();
    let environment = EnvironmentConfig::from_pairs(platform, values).unwrap();
    let runtime = RuntimeConfiguration::from_environment(&environment)
        .await
        .unwrap();
    RelayKnowledgeService::with_store(
        runtime,
        Arc::new(SqliteGraphStore::open_in_memory().unwrap()),
    )
}
