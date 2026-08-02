//! Direct unit contract for forward lifecycle step sequencing.

use super::super::*;
use crate::env::{EnvironmentConfig, PlatformKind};

#[test]
fn forward_plans_keep_actions_and_mutating_steps_distinct() {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            ("RELAY_KNOWLEDGE_HOME", "/tmp/relay-knowledge-forward-steps"),
        ],
    )
    .expect("environment");
    let paths =
        RuntimePaths::resolve(&environment.platform, &environment.paths).expect("runtime paths");
    let mut request = ServicePlanRequest {
        action: ServiceManagerAction::Install,
        dry_run: true,
        execute: false,
        target_version: None,
        install_dir: Some("/opt/relay".to_owned()),
    };
    let install = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/relay-knowledge"),
    )
    .expect("install plan");
    request.action = ServiceManagerAction::Uninstall;
    let uninstall = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/relay-knowledge"),
    )
    .expect("uninstall plan");

    assert!(
        install
            .lifecycle_steps
            .iter()
            .any(|step| step.id == "copy-binary")
    );
    assert!(
        uninstall
            .lifecycle_steps
            .iter()
            .any(|step| step.id == "remove-service-definition")
    );
    assert!(
        uninstall
            .lifecycle_steps
            .iter()
            .all(|step| step.id != "copy-binary")
    );
}
