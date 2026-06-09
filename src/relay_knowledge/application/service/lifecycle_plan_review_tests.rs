use super::*;
use crate::env::{EnvironmentConfig, PathEnvOverrides, PlatformEnvironment, PlatformKind};
use std::time::Duration;

#[test]
fn macos_install_writes_plist_under_launch_agents() {
    let environment = PlatformEnvironment {
        platform: PlatformKind::Macos,
        home_dir: Some(PathBuf::from("/Users/alice")),
        xdg_config_home: None,
        xdg_data_home: None,
        xdg_state_home: None,
        xdg_cache_home: None,
        xdg_runtime_dir: None,
        app_data: None,
        local_app_data: None,
        temp_dir: None,
    };
    let paths =
        RuntimePaths::resolve(&environment, &PathEnvOverrides::default()).expect("mac paths");
    let request = request(ServiceManagerAction::Install);

    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "macos",
        PathBuf::from("/Applications/relay-knowledge"),
    )
    .expect("plan should render");

    assert_eq!(
        plan.definition_path,
        "/Users/alice/Library/LaunchAgents/com.coolplayagent.relay-knowledge.plist"
    );
    assert!(
        plan.install_command
            .iter()
            .any(|part| part == &plan.definition_path)
    );
}

#[test]
fn install_rollback_reloads_systemd_after_removing_definition() {
    let paths = runtime_paths();
    let request = request(ServiceManagerAction::Install);
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/bin/relay-knowledge"),
    )
    .expect("plan should render");
    let rollback_ids: Vec<&str> = plan
        .rollback_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect();

    assert!(
        rollback_ids
            .iter()
            .position(|id| *id == "remove-service-definition")
            < rollback_ids
                .iter()
                .position(|id| *id == "reload-service-manager")
    );
}

#[test]
fn upgrade_checkpoint_capture_failure_does_not_stop_service() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Upgrade);
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
    let mut runner = FailingRunner {
        fail_step: "capture-rollback-checkpoint",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("capture-rollback-checkpoint")
    );
    assert!(report.rollback_steps.is_empty());
    assert!(
        runner
            .calls
            .iter()
            .all(|step| step != "stop-service" && step != "restore-service-definition")
    );
}

#[test]
fn install_rejects_existing_service_definition_before_writing_files() {
    let root = unique_root("existing-service-definition");
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    std::fs::write(&definition_path, b"old definition")
        .expect("existing definition should be written");
    let mut request = request(ServiceManagerAction::Install);
    request.dry_run = false;
    request.execute = true;
    request.install_dir = Some(root.join("bin").display().to_string());
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    let step_ids: Vec<String> = plan
        .lifecycle_steps
        .iter()
        .map(|step| step.id.clone())
        .collect();
    plan.lifecycle_steps
        .retain(|step| step.id == "verify-service-definition-target");
    let mut runner = ProcessStepRunner;

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(
        step_ids
            .iter()
            .position(|id| id == "verify-service-definition-target")
            < step_ids.iter().position(|id| id == "copy-binary")
    );
    assert!(
        step_ids
            .iter()
            .position(|id| id == "verify-service-definition-target")
            < step_ids
                .iter()
                .position(|id| id == "write-service-definition")
    );
    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("verify-service-definition-target")
    );
    assert!(report.rollback_steps.is_empty());
    assert_eq!(
        std::fs::read(&definition_path).expect("definition should remain"),
        b"old definition"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn windows_stop_command_stops_on_service_stop_errors() {
    let paths = runtime_paths();
    let request = request(ServiceManagerAction::Uninstall);
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "windows",
        PathBuf::from("/tmp/current/relay-knowledge.exe"),
    )
    .expect("plan should render");
    let script = plan
        .stop_command
        .last()
        .expect("powershell stop script should exist");
    let stop_step = plan
        .lifecycle_steps
        .iter()
        .find(|step| step.id == "stop-service")
        .expect("stop-service step should exist");

    assert!(script.contains("$ErrorActionPreference = 'Stop'"));
    assert!(script.contains("Stop-Service"));
    assert!(script.contains("-ErrorAction Stop"));
    assert_eq!(stop_step.command, plan.stop_command);
}

#[test]
fn uninstall_rollback_recreates_definition_before_registration() {
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
    let rollback_ids: Vec<&str> = plan
        .rollback_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    let mut runner = FailingRunner {
        fail_step: "reload-service-manager",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(
        rollback_ids
            .iter()
            .position(|id| *id == "restore-service-definition")
            < rollback_ids.iter().position(|id| *id == "install-service")
    );
    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("reload-service-manager")
    );
    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "restore-service-definition")
    );
    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "install-service")
    );
}

#[test]
fn upgrade_stop_failure_does_not_restore_or_start_service() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Upgrade);
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
    let mut runner = FailingRunner {
        fail_step: "stop-service",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id.as_deref(), Some("stop-service"));
    assert!(report.rollback_steps.is_empty());
    assert!(
        runner
            .calls
            .iter()
            .all(|step| step != "restore-service-definition" && step != "start-service")
    );
}

#[test]
fn upgrade_rollback_removes_definition_when_checkpoint_has_no_backup() {
    let root = unique_root("upgrade-no-definition-backup");
    let _ = std::fs::remove_dir_all(&root);
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    std::fs::write(&definition_path, b"new definition").expect("definition should be written");
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "upgrade",
        "binary_path": root.join("bin").join(PROJECT_NAME),
        "definition_path": definition_path,
        "checksum": "abc123",
        "definition_backup_path": null,
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
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    plan.lifecycle_steps
        .retain(|step| step.id == "restore-service-definition");
    let mut runner = ProcessStepRunner;

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id, None);
    assert!(!definition_path.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_rollback_removes_no_backup_upgrade_binary_without_install_dir_flag() {
    let root = unique_root("explicit-no-backup-binary");
    let _ = std::fs::remove_dir_all(&root);
    let paths = runtime_paths_at(&root);
    let install_dir = root.join("bin");
    let binary_path = install_dir.join(PROJECT_NAME);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    std::fs::create_dir_all(&install_dir).expect("install dir should be created");
    std::fs::write(&binary_path, b"new binary").expect("binary should be written");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    let definition_backup = paths
        .service_dir
        .join("relay-knowledge.service.definition.rollback");
    std::fs::write(&definition_backup, b"old definition")
        .expect("definition backup should be written");
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "upgrade",
        "binary_path": binary_path,
        "definition_path": definition_path,
        "checksum": "abc123",
        "definition_backup_path": definition_backup,
        "binary_backup_path": null,
        "binary_cleanup_on_no_backup": true,
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
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");

    assert!(plan.lifecycle_steps.iter().any(|step| {
        step.id == "restore-binary"
            && step
                .writes_paths
                .contains(&binary_path.display().to_string())
    }));

    plan.lifecycle_steps
        .retain(|step| step.id == "restore-binary");
    let mut runner = ProcessStepRunner;

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id, None);
    assert!(!binary_path.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn external_lifecycle_commands_drain_large_output_while_waiting() {
    let command = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        "yes lifecycle-output | head -c 200000".to_owned(),
    ];

    let result = run_command_with_timeout(&command, Duration::from_secs(5));

    assert_eq!(result.as_deref(), Ok("exit_status=exit status: 0"));
}

#[test]
fn windows_install_environment_failure_unregisters_created_service() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
    request.dry_run = false;
    request.execute = true;
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "windows",
        PathBuf::from("/tmp/current/relay-knowledge.exe"),
    )
    .expect("plan should render");
    let step_ids: Vec<&str> = plan
        .lifecycle_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    let install_script = plan
        .install_command
        .last()
        .expect("Windows install script should exist");
    let configure_script = plan
        .lifecycle_steps
        .iter()
        .find(|step| step.id == "configure-service-environment")
        .and_then(|step| step.command.last())
        .expect("configure script should exist");
    let mut runner = FailingRunner {
        fail_step: "configure-service-environment",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(
        step_ids.iter().position(|id| *id == "install-service")
            < step_ids
                .iter()
                .position(|id| *id == "configure-service-environment")
    );
    assert!(
        step_ids
            .iter()
            .position(|id| *id == "configure-service-environment")
            < step_ids.iter().position(|id| *id == "start-service")
    );
    assert!(install_script.contains("New-Service"));
    assert!(!install_script.contains("New-ItemProperty"));
    assert!(configure_script.contains("New-ItemProperty"));
    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("configure-service-environment")
    );
    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "uninstall-service")
    );
}

#[test]
fn failed_restore_rollback_skips_reload_and_restart() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Upgrade);
    request.dry_run = false;
    request.execute = true;
    request.install_dir = Some("/opt/relay".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    let mut runner = DualFailingRunner {
        lifecycle_fail_step: "post-upgrade-doctor",
        rollback_fail_step: "restore-service-definition",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("post-upgrade-doctor")
    );
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
            .any(|step| step.step_id == "reload-service-manager" || step.step_id == "start-service")
    );
    let restore_call = runner
        .calls
        .iter()
        .position(|step| step == "restore-service-definition")
        .expect("restore should be attempted");
    assert!(
        runner.calls[restore_call + 1..]
            .iter()
            .all(|step| step != "reload-service-manager" && step != "start-service")
    );
}

#[test]
fn windows_upgrade_refreshes_service_registration_before_start() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Upgrade);
    request.install_dir = Some("/Program Files/relay knowledge".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "windows",
        PathBuf::from("/tmp/current/relay-knowledge.exe"),
    )
    .expect("plan should render");
    let step_ids: Vec<&str> = plan
        .lifecycle_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    let refresh_script = plan
        .lifecycle_steps
        .iter()
        .find(|step| step.id == "refresh-service-registration")
        .and_then(|step| step.command.last())
        .expect("refresh script should exist");

    assert!(
        step_ids
            .iter()
            .position(|id| *id == "write-service-definition")
            < step_ids
                .iter()
                .position(|id| *id == "refresh-service-registration")
    );
    assert!(
        step_ids
            .iter()
            .position(|id| *id == "refresh-service-registration")
            < step_ids.iter().position(|id| *id == "start-service")
    );
    assert!(refresh_script.contains("Get-Content -Raw -Path"));
    assert!(refresh_script.contains("sc.exe config"));
    assert!(refresh_script.contains("binPath="));
    assert!(refresh_script.contains("New-ItemProperty"));
    assert!(refresh_script.contains(&plan.definition_path));
}

#[test]
fn macos_upgrade_reloads_launchd_registration_before_start() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Upgrade);
    request.install_dir = Some("/Applications/Relay Knowledge".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "macos",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    let step_ids: Vec<&str> = plan
        .lifecycle_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect();

    assert!(
        step_ids
            .iter()
            .position(|id| *id == "write-service-definition")
            < step_ids
                .iter()
                .position(|id| *id == "unload-service-registration")
    );
    assert!(
        step_ids
            .iter()
            .position(|id| *id == "unload-service-registration")
            < step_ids
                .iter()
                .position(|id| *id == "load-service-registration")
    );
    assert!(
        step_ids
            .iter()
            .position(|id| *id == "load-service-registration")
            < step_ids.iter().position(|id| *id == "start-service")
    );
}

#[test]
fn explicit_rollback_accepts_no_backup_upgrade_definition_checkpoint() {
    let root = unique_root("explicit-no-definition-backup");
    let _ = std::fs::remove_dir_all(&root);
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    std::fs::write(&definition_path, b"new definition").expect("definition should be written");
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "upgrade",
        "binary_path": root.join("bin").join(PROJECT_NAME),
        "definition_path": definition_path,
        "checksum": "abc123",
        "definition_backup_path": null,
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
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    plan.lifecycle_steps.retain(|step| {
        step.id == "validate-rollback-checkpoint" || step.id == "restore-service-definition"
    });
    let mut runner = ProcessStepRunner;

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id, None);
    assert!(
        report
            .completed_steps
            .iter()
            .any(|step| step.step_id == "validate-rollback-checkpoint")
    );
    assert!(!definition_path.exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn linux_service_definition_escapes_literal_dollars() {
    let paths = runtime_paths_at(Path::new("/tmp/relay$knowledge-lifecycle"));
    let mut request = request(ServiceManagerAction::Install);
    request.install_dir = Some("/opt/relay$prod".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");

    assert!(plan.definition.contains(
        "ExecStart=\"/opt/relay$$prod/relay-knowledge\" service run --web --mcp streamable-http"
    ));
    assert!(
        plan.definition.contains(
            "Environment=\"RELAY_KNOWLEDGE_DATA_DIR=/tmp/relay$$knowledge-lifecycle/data\""
        )
    );
}

#[test]
fn install_rollback_keeps_files_when_unregister_fails() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
    request.dry_run = false;
    request.execute = true;
    request.install_dir = Some("/opt/relay".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    let mut runner = DualFailingRunner {
        lifecycle_fail_step: "start-service",
        rollback_fail_step: "uninstall-service",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id.as_deref(), Some("start-service"));
    let unregister = report
        .rollback_steps
        .iter()
        .position(|step| step.step_id == "uninstall-service" && step.status == "failed")
        .expect("unregister rollback failure should be reported");
    assert!(
        report.rollback_steps[unregister + 1..]
            .iter()
            .all(|step| step.step_id != "remove-service-definition"
                && step.step_id != "remove-installed-binary"
                && step.step_id != "reload-service-manager")
    );
    let unregister_call = runner
        .calls
        .iter()
        .position(|step| step == "uninstall-service")
        .expect("unregister should be attempted");
    assert!(
        runner.calls[unregister_call + 1..]
            .iter()
            .all(|step| step != "remove-service-definition"
                && step != "remove-installed-binary"
                && step != "reload-service-manager")
    );
}

fn runtime_paths() -> RuntimePaths {
    runtime_paths_at(Path::new(
        "/tmp/relay-knowledge-lifecycle-review-tests/default",
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
        "relay-knowledge-lifecycle-review-{name}-{}",
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

struct FailingRunner {
    fail_step: &'static str,
    calls: Vec<String>,
}

struct DualFailingRunner {
    lifecycle_fail_step: &'static str,
    rollback_fail_step: &'static str,
    calls: Vec<String>,
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

impl StepRunner for FailingRunner {
    fn run(
        &mut self,
        _plan: &ServiceDefinitionPlan,
        step: &ServiceLifecycleStep,
    ) -> Result<String, String> {
        self.calls.push(step.id.clone());
        if step.id == self.fail_step {
            Err("forced failure".to_owned())
        } else {
            Ok("ok".to_owned())
        }
    }
}
