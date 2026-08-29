use std::path::PathBuf;

use super::*;
use crate::{api::ServicePlanRequest, domain::ServiceManagerAction, env::PlatformKind};

#[tokio::test]
async fn lifecycle_plan_uses_executable_captured_by_bootstrap() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/tmp"),
            (
                "RELAY_KNOWLEDGE_HOME",
                "/tmp/relay-knowledge-bootstrap-test",
            ),
        ],
    )
    .expect("environment should parse");
    let executable = PathBuf::from("/captured/relay-knowledge");
    let service = RelayKnowledgeService::from_environment_with_process(
        &environment,
        ProcessRuntimeConfig::from_bootstrap_inputs(executable.clone(), None),
    )
    .await
    .expect("runtime should resolve");
    let plan = service
        .render_service_plan_for_request(&ServicePlanRequest {
            action: ServiceManagerAction::Install,
            dry_run: true,
            execute: false,
            target_version: None,
            install_dir: Some("/opt/relay-knowledge".to_owned()),
        })
        .expect("plan should render");

    let preflight = plan
        .lifecycle_steps
        .iter()
        .find(|step| step.id == "preflight-doctor")
        .expect("preflight should exist");
    assert_eq!(
        preflight.command.first(),
        Some(&executable.display().to_string())
    );
}
