//! Restored service storage admission without executing service-manager commands.

use super::*;
use crate::env::{EnvironmentConfig, PathEnvOverrides, PlatformKind, RELAY_KNOWLEDGE_HOME};
use std::path::PathBuf;

fn fixture() -> (PathBuf, RuntimePaths, ServiceDefinitionPlan) {
    let root = std::env::temp_dir().join(format!(
        "relay-checkpoint-storage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::current(),
        [(RELAY_KNOWLEDGE_HOME, root.to_str().unwrap())],
    )
    .unwrap();
    let paths = RuntimePaths::resolve(&environment.platform, &environment.paths).unwrap();
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &ServicePlanRequest {
            action: ServiceManagerAction::Upgrade,
            dry_run: false,
            execute: true,
            target_version: None,
            install_dir: None,
        },
        "windows",
        root.join("relay-knowledge.exe"),
    )
    .unwrap();
    plan.lifecycle_steps.clear();
    plan.rollback_steps.clear();
    std::fs::create_dir_all(&paths.service_dir).unwrap();
    (root, paths, plan)
}

#[tokio::test]
async fn upgrade_and_rollback_validate_old_storage_before_execution() {
    let (root, current, mut plan) = fixture();
    validate_restored_storage(&plan, &current).await.unwrap(); // No prior installation.
    let old = current
        .with_service_storage_overrides(&PathEnvOverrides {
            data_dir: Some(root.join("old-data")),
            ..Default::default()
        })
        .unwrap();
    let definition = platform_service::render_definition(
        "windows",
        &plan.binary_path,
        old.data_dir.to_str().unwrap(),
        StorageTopology::SingleSqlite,
    );
    std::fs::write(&plan.definition_path, &definition).unwrap();
    std::fs::create_dir_all(&current.data_dir).unwrap();
    std::fs::write(current.database_file(), []).unwrap();
    assert!(
        validate_restored_storage(&plan, &current)
            .await
            .unwrap_err()
            .contains("checkpointed service database is missing")
    );
    assert!(
        !old.data_dir.exists(),
        "preflight must not provision restored storage"
    );
    std::fs::create_dir_all(&old.data_dir).unwrap();
    std::fs::write(old.database_file(), []).unwrap();
    validate_restored_storage(&plan, &current).await.unwrap();
    checkpoint::capture_checkpoint(&plan).unwrap();
    std::fs::write(&plan.definition_path, &plan.definition).unwrap();
    plan.action = ServiceManagerAction::Rollback;
    validate_restored_storage(&plan, &current).await.unwrap();
    std::fs::remove_file(old.database_file()).unwrap();
    assert!(
        validate_restored_storage(&plan, &current)
            .await
            .unwrap_err()
            .contains("checkpointed service database is missing")
    );
    plan.dry_run = true;
    validate_restored_storage(&plan, &current).await.unwrap();
    plan.dry_run = false;
    plan.action = ServiceManagerAction::Uninstall;
    validate_restored_storage(&plan, &current).await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn oversized_checkpointed_definition_is_rejected() {
    let (root, _, plan) = fixture();
    std::fs::write(&plan.definition_path, vec![b' '; 65_537]).unwrap();
    assert!(
        checkpoint::read_bounded_definition(Path::new(&plan.definition_path))
            .unwrap_err()
            .contains("exceeds")
    );
    std::fs::remove_dir_all(root).unwrap();
}
