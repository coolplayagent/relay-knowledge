use super::*;
use crate::env::{EnvironmentConfig, PlatformKind};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[test]
fn uninstall_rollback_restores_removed_definition_from_checkpoint() {
    let root = unique_root("uninstall-restore-definition");
    let _ = std::fs::remove_dir_all(&root);
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    std::fs::write(&definition_path, b"installed definition")
        .expect("installed definition should be written");
    let mut request = request(ServiceManagerAction::Uninstall);
    request.dry_run = false;
    request.execute = true;
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/bin/relay-knowledge"),
    )
    .expect("plan should render");
    let mut runner = ProcessBackedFailingRunner {
        fail_step: "reload-service-manager",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("reload-service-manager")
    );
    assert_eq!(
        std::fs::read(&definition_path).expect("definition should be restored"),
        b"installed definition"
    );
    let restore = runner
        .calls
        .iter()
        .position(|step| step == "restore-service-definition")
        .expect("definition should be restored");
    assert!(
        restore
            < runner
                .calls
                .iter()
                .position(|step| step == "install-service")
                .expect("service should be re-registered")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn uninstall_rollback_stops_after_definition_restore_failure() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Uninstall);
    request.dry_run = false;
    request.execute = true;
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/bin/relay-knowledge"),
    )
    .expect("plan should render");
    let mut runner = DualFailingRunner {
        lifecycle_fail_step: "reload-service-manager",
        rollback_fail_step: "restore-service-definition",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "restore-service-definition" && step.status == "failed")
    );
    assert!(
        !report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "install-service" || step.step_id == "start-service")
    );
}

#[test]
fn explicit_rollback_for_uninstall_checkpoint_re_registers_service() {
    let root = unique_root("explicit-uninstall-rollback");
    let _ = std::fs::remove_dir_all(&root);
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    let definition_backup = paths
        .service_dir
        .join("relay-knowledge.service.definition.rollback");
    std::fs::write(&definition_backup, b"removed definition")
        .expect("definition backup should be written");
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "uninstall",
        "binary_path": root.join("bin").join(PROJECT_NAME),
        "definition_path": definition_path,
        "checksum": "abc123",
        "definition_backup_path": definition_backup,
        "binary_backup_path": null,
    });
    std::fs::write(
        paths
            .service_dir
            .join(SERVICE_LIFECYCLE_CHECKPOINT_FILE_NAME),
        serde_json::to_string(&checkpoint).expect("checkpoint should serialize"),
    )
    .expect("checkpoint should be written");
    let mut request = request(ServiceManagerAction::Rollback);
    request.dry_run = false;
    request.execute = true;
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/bin/relay-knowledge"),
    )
    .expect("plan should render");
    let step_ids = plan
        .lifecycle_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(step_ids.first(), Some(&"validate-rollback-checkpoint"));
    let restore = step_ids
        .iter()
        .position(|step| *step == "restore-service-definition")
        .expect("definition should be restored");
    let install = step_ids
        .iter()
        .position(|step| *step == "install-service")
        .expect("service should be re-registered");
    assert!(restore < install);
    assert!(step_ids.contains(&"start-service"));
    assert!(!step_ids.contains(&"refresh-service-registration"));
    assert!(!step_ids.contains(&"reload-service-manager"));
    assert!(!step_ids.contains(&"stop-service"));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn lifecycle_command_output_join_is_bounded_for_inherited_pipes() {
    let command = vec!["sh".to_owned(), "-c".to_owned(), "sleep 3 &".to_owned()];
    let started = Instant::now();

    let result = run_command_with_timeout(&command, Duration::from_secs(2));

    assert_eq!(result.as_deref(), Ok("exit_status=exit status: 0"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn definition_only_upgrade_checkpoint_does_not_restore_binary() {
    let root = unique_root("definition-only-upgrade");
    let _ = std::fs::remove_dir_all(&root);
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let current_binary = root.join("current").join(PROJECT_NAME);
    let definition_backup = paths
        .service_dir
        .join("relay-knowledge.service.definition.rollback");
    std::fs::write(&definition_backup, b"old definition")
        .expect("definition backup should be written");
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "upgrade",
        "binary_path": current_binary,
        "definition_path": paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME),
        "checksum": "abc123",
        "definition_backup_path": definition_backup,
        "binary_backup_path": null,
    });
    std::fs::write(
        paths
            .service_dir
            .join(SERVICE_LIFECYCLE_CHECKPOINT_FILE_NAME),
        serde_json::to_string(&checkpoint).expect("checkpoint should serialize"),
    )
    .expect("checkpoint should be written");
    let mut request = request(ServiceManagerAction::Rollback);
    request.dry_run = false;
    request.execute = true;
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");

    assert!(
        plan.lifecycle_steps
            .iter()
            .all(|step| step.id != "restore-binary")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn checkpoint_capture_does_not_overwrite_previous_backup() {
    let root = unique_root("attempt-scoped-backup");
    let _ = std::fs::remove_dir_all(&root);
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    let mut request = request(ServiceManagerAction::Upgrade);
    request.dry_run = false;
    request.execute = true;
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/bin/relay-knowledge"),
    )
    .expect("plan should render");
    plan.lifecycle_steps
        .retain(|step| step.id == "capture-rollback-checkpoint");

    std::fs::write(&definition_path, b"old definition v1")
        .expect("first definition should be written");
    let mut runner = ProcessStepRunner;
    let first_report = execute_service_plan_blocking(&plan, &mut runner);
    assert_eq!(first_report.failed_step_id, None);
    let first_backup = checkpoint_definition_backup(&plan);

    std::fs::write(&definition_path, b"old definition v2")
        .expect("second definition should be written");
    let mut runner = ProcessStepRunner;
    let second_report = execute_service_plan_blocking(&plan, &mut runner);
    assert_eq!(second_report.failed_step_id, None);
    let second_backup = checkpoint_definition_backup(&plan);

    assert_ne!(first_backup, second_backup);
    assert_eq!(
        std::fs::read(&first_backup).expect("first backup should remain"),
        b"old definition v1"
    );
    assert_eq!(
        std::fs::read(&second_backup).expect("second backup should exist"),
        b"old definition v2"
    );
    let _ = std::fs::remove_dir_all(root);
}

fn checkpoint_definition_backup(plan: &ServiceDefinitionPlan) -> PathBuf {
    let checkpoint: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&plan.checkpoint_path).expect("checkpoint should be readable"),
    )
    .expect("checkpoint should parse");
    PathBuf::from(
        checkpoint["definition_backup_path"]
            .as_str()
            .expect("definition backup should be recorded"),
    )
}

fn runtime_paths() -> RuntimePaths {
    runtime_paths_at(Path::new(
        "/tmp/relay-knowledge-lifecycle-review-followup/default",
    ))
}

fn runtime_paths_at(root: &Path) -> RuntimePaths {
    let environment = EnvironmentConfig::from_pairs(
        PlatformKind::Unix,
        [
            ("HOME", "/home/alice"),
            (
                "RELAY_KNOWLEDGE_HOME",
                root.to_str().expect("test root should be utf-8"),
            ),
        ],
    )
    .expect("environment should parse");
    RuntimePaths::resolve(&environment.platform, &environment.paths).expect("paths should resolve")
}

fn unique_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "relay-knowledge-lifecycle-review-followup-{name}-{}",
        std::process::id()
    ))
}

fn request(action: ServiceManagerAction) -> ServicePlanRequest {
    ServicePlanRequest {
        action,
        dry_run: true,
        execute: false,
        target_version: None,
        install_dir: None,
    }
}

struct DualFailingRunner {
    lifecycle_fail_step: &'static str,
    rollback_fail_step: &'static str,
    calls: Vec<String>,
}

struct ProcessBackedFailingRunner {
    fail_step: &'static str,
    calls: Vec<String>,
}

impl StepRunner for ProcessBackedFailingRunner {
    fn run(
        &mut self,
        plan: &ServiceDefinitionPlan,
        step: &ServiceLifecycleStep,
    ) -> Result<String, String> {
        self.calls.push(step.id.clone());
        match step.id.as_str() {
            "capture-rollback-checkpoint"
            | "remove-service-definition"
            | "restore-service-definition" => ProcessStepRunner.run(plan, step),
            id if id == self.fail_step => Err("forced failure".to_owned()),
            _ => Ok("ok".to_owned()),
        }
    }
}

impl StepRunner for DualFailingRunner {
    fn run(
        &mut self,
        _plan: &ServiceDefinitionPlan,
        step: &ServiceLifecycleStep,
    ) -> Result<String, String> {
        self.calls.push(step.id.clone());
        let lifecycle_failed = self
            .calls
            .iter()
            .any(|call| call == self.lifecycle_fail_step);
        if step.id == self.lifecycle_fail_step
            || (lifecycle_failed && step.id == self.rollback_fail_step)
        {
            Err("forced failure".to_owned())
        } else {
            Ok("ok".to_owned())
        }
    }
}
