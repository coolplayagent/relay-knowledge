use super::*;
use crate::domain::ServiceLifecycleStepResult;
use crate::env::{EnvironmentConfig, PlatformKind};
use std::time::Duration;

#[test]
fn lifecycle_plans_cover_supported_platform_commands() {
    let paths = runtime_paths();
    for (platform, definition_file, manager_command) in [
        ("linux", LINUX_SERVICE_DEFINITION_FILE_NAME, "systemctl"),
        ("macos", MACOS_SERVICE_DEFINITION_FILE_NAME, "launchctl"),
        (
            "windows",
            WINDOWS_SERVICE_DEFINITION_FILE_NAME,
            "powershell",
        ),
    ] {
        let request = request(ServiceManagerAction::Install);
        let plan = render_service_plan_for_platform(
            &paths,
            StorageTopology::SingleSqlite,
            &request,
            platform,
            PathBuf::from("/bin/relay-knowledge"),
        )
        .expect("plan should render");

        assert_eq!(plan.platform, platform);
        assert!(plan.definition_path.ends_with(definition_file));
        assert!(
            plan.install_command
                .iter()
                .any(|part| part == manager_command)
        );
        assert!(!plan.lifecycle_steps.is_empty());
        assert!(!plan.rollback_steps.is_empty());
        assert!(!plan.permission_requirements.is_empty());
        assert_eq!(plan.package_manifest_checks.len(), 4);
    }
}

#[test]
fn upgrade_plan_records_install_dir_version_and_runtime_shards() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Upgrade);
    request.target_version = Some("1.2.3".to_owned());
    request.install_dir = Some("/opt/relay".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::PartitionedSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");

    assert_eq!(plan.target_version.as_deref(), Some("1.2.3"));
    assert_eq!(plan.install_dir.as_deref(), Some("/opt/relay"));
    assert_eq!(plan.binary_path, "/opt/relay/relay-knowledge");
    assert!(
        plan.runtime_state_paths
            .iter()
            .any(|path| path.contains("repositories"))
    );
    assert!(
        plan.lifecycle_steps
            .iter()
            .any(|step| step.id == "capture-rollback-checkpoint")
    );
}

#[test]
fn install_dir_install_copies_binary_after_source_preflight() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
    request.install_dir = Some("/opt/relay".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");

    let step_ids: Vec<&str> = plan
        .lifecycle_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    let preflight = plan
        .lifecycle_steps
        .iter()
        .find(|step| step.id == "preflight-doctor")
        .expect("preflight should exist");

    assert_eq!(
        preflight.command.first().map(String::as_str),
        Some("/tmp/current/relay-knowledge")
    );
    assert!(
        step_ids.iter().position(|id| *id == "preflight-doctor")
            < step_ids.iter().position(|id| *id == "copy-binary")
    );
    assert!(
        step_ids
            .iter()
            .position(|id| *id == "verify-install-target")
            < step_ids.iter().position(|id| *id == "copy-binary")
    );
    assert!(
        step_ids.iter().position(|id| *id == "copy-binary")
            < step_ids
                .iter()
                .position(|id| *id == "write-service-definition")
    );
    assert!(
        plan.rollback_steps
            .iter()
            .any(|step| step.id == "remove-installed-binary")
    );
}

#[test]
fn linux_install_registers_generated_unit_path() {
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

    assert!(!plan.install_command.iter().any(|part| part == "--now"));
    assert!(
        plan.install_command
            .iter()
            .any(|part| part == &plan.definition_path)
    );
    assert!(
        plan.lifecycle_steps
            .iter()
            .any(|step| step.id == "start-service")
    );
}

#[test]
fn linux_uninstall_reloads_systemd_after_removing_definition() {
    let paths = runtime_paths();
    let request = request(ServiceManagerAction::Uninstall);
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/bin/relay-knowledge"),
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
            .position(|id| *id == "remove-service-definition")
            < step_ids
                .iter()
                .position(|id| *id == "reload-service-manager")
    );
}

#[test]
fn linux_service_definition_quotes_paths_with_spaces() {
    let paths = runtime_paths_at(Path::new("/tmp/relay knowledge lifecycle"));
    let mut request = request(ServiceManagerAction::Install);
    request.install_dir = Some("/opt/relay knowledge".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");

    assert!(plan.definition.contains(
        "ExecStart=\"/opt/relay knowledge/relay-knowledge\" service run --web --mcp streamable-http"
    ));
    assert!(
        plan.definition.contains(
            "Environment=\"RELAY_KNOWLEDGE_DATA_DIR=/tmp/relay knowledge lifecycle/data\""
        )
    );
}

#[test]
fn macos_service_definition_preserves_runtime_data_dir() {
    let paths = runtime_paths();
    let request = request(ServiceManagerAction::Install);
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "macos",
        PathBuf::from("/Applications/relay-knowledge"),
    )
    .expect("plan should render");

    assert!(plan.definition.contains("EnvironmentVariables"));
    assert!(plan.definition.contains("RELAY_KNOWLEDGE_DATA_DIR"));
    assert!(
        plan.definition
            .contains(&paths.data_dir.display().to_string())
    );
}

#[test]
fn windows_install_command_quotes_binary_and_plans_environment_step() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
    request.install_dir = Some("/Program Files/relay knowledge".to_owned());
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "windows",
        PathBuf::from("/tmp/current/relay-knowledge.exe"),
    )
    .expect("plan should render");
    let script = plan
        .install_command
        .last()
        .expect("powershell script should exist");

    assert!(script.contains(
        "-BinaryPathName '\"/Program Files/relay knowledge/relay-knowledge.exe\" service run --web --mcp streamable-http'"
    ));
    assert!(script.contains("$ErrorActionPreference = 'Stop'"));
    assert!(script.contains("-ErrorAction Stop"));
    assert!(!script.contains("New-ItemProperty"));
    let configure_step = plan
        .lifecycle_steps
        .iter()
        .find(|step| step.id == "configure-service-environment")
        .expect("Windows install should configure service environment separately");
    let configure_script = configure_step
        .command
        .last()
        .expect("PowerShell configure script should exist");

    assert!(configure_script.contains("New-ItemProperty"));
    assert!(configure_script.contains("RELAY_KNOWLEDGE_DATA_DIR=$dataDir"));
    assert!(configure_script.contains(&plan.definition_path));
}

#[test]
fn windows_start_command_stops_on_service_start_errors() {
    let paths = runtime_paths();
    let request = request(ServiceManagerAction::Install);
    let plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "windows",
        PathBuf::from("/tmp/current/relay-knowledge.exe"),
    )
    .expect("plan should render");
    let script = plan
        .start_command
        .last()
        .expect("powershell start script should exist");
    let start_step = plan
        .lifecycle_steps
        .iter()
        .find(|step| step.id == "start-service")
        .expect("start-service step should exist");

    assert!(script.contains("$ErrorActionPreference = 'Stop'"));
    assert!(script.contains("Start-Service"));
    assert!(script.contains("-ErrorAction Stop"));
    assert_eq!(start_step.command, plan.start_command);
}

#[test]
fn windows_uninstall_uses_sc_delete_for_powershell_compatibility() {
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

    assert_eq!(
        plan.uninstall_command,
        vec!["sc.exe", "delete", PROJECT_NAME]
    );
}

#[test]
fn service_plan_request_json_execute_defaults_to_non_dry_run() {
    let request: ServicePlanRequest = serde_json::from_value(serde_json::json!({
        "action": "install",
        "execute": true
    }))
    .expect("request should deserialize");

    assert!(request.execute);
    assert!(!request.dry_run);
}

#[test]
fn service_plan_request_json_defaults_to_dry_run_without_execute() {
    let request: ServicePlanRequest = serde_json::from_value(serde_json::json!({
        "action": "install"
    }))
    .expect("request should deserialize");

    assert!(!request.execute);
    assert!(request.dry_run);
}

#[test]
fn invalid_install_dir_is_rejected() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
    request.install_dir = Some("../relay".to_owned());

    let error = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/bin/relay-knowledge"),
    )
    .expect_err("relative install dir should fail");

    assert!(error.contains("absolute path"));
}

#[test]
fn failed_execution_runs_rollback_steps() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
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
        fail_step: "start-service",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(report.executed);
    assert!(report.rolled_back);
    assert_eq!(report.failed_step_id.as_deref(), Some("start-service"));
    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "uninstall-service")
    );
    assert!(
        runner
            .calls
            .iter()
            .any(|step| step == "remove-service-definition")
    );
}

#[test]
fn failed_install_registration_does_not_uninstall_existing_service() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
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
        fail_step: "install-service",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id.as_deref(), Some("install-service"));
    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "remove-service-definition")
    );
    assert!(
        report
            .rollback_steps
            .iter()
            .all(|step| step.step_id != "uninstall-service" && step.step_id != "stop-service")
    );
}

#[test]
fn failed_install_target_verification_preserves_existing_binary() {
    let root = unique_root("existing-install-binary");
    let install_dir = root.join("bin");
    std::fs::create_dir_all(&install_dir).expect("install dir should be created");
    let binary_path = install_dir.join(PROJECT_NAME);
    std::fs::write(&binary_path, b"old binary").expect("existing binary should be written");
    let paths = runtime_paths_at(&root);
    let mut request = request(ServiceManagerAction::Install);
    request.dry_run = false;
    request.execute = true;
    request.install_dir = Some(install_dir.display().to_string());
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    plan.lifecycle_steps
        .retain(|step| step.id == "verify-install-target");
    let mut runner = ProcessStepRunner;

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("verify-install-target")
    );
    assert!(report.rollback_steps.is_empty());
    assert_eq!(
        std::fs::read(&binary_path).expect("existing binary should remain"),
        b"old binary"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn failed_initial_uninstall_stop_does_not_reinstall_service() {
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
    let mut runner = FailingRunner {
        fail_step: "stop-service",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id.as_deref(), Some("stop-service"));
    assert!(!report.rolled_back);
    assert!(report.rollback_steps.is_empty());
}

#[test]
fn failed_uninstall_after_stop_restarts_without_reinstalling() {
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
    let mut runner = FailingRunner {
        fail_step: "uninstall-service",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id.as_deref(), Some("uninstall-service"));
    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "start-service")
    );
    assert!(
        report
            .rollback_steps
            .iter()
            .all(|step| step.step_id != "install-service")
    );
}

#[test]
fn preflight_failure_does_not_run_rollback_before_mutation() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
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
        fail_step: "preflight-doctor",
        calls: Vec::new(),
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(report.executed);
    assert!(!report.rolled_back);
    assert!(report.rollback_steps.is_empty());
    assert_eq!(report.failed_step_id.as_deref(), Some("preflight-doctor"));
    assert_eq!(runner.calls, vec!["preflight-doctor"]);
}

#[test]
fn rollback_failure_is_reported_at_top_level() {
    let paths = runtime_paths();
    let mut request = request(ServiceManagerAction::Install);
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
    let mut runner = RollbackFailingRunner {
        lifecycle_fail_step: "start-service",
        rollback_fail_step: "uninstall-service",
    };

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(!report.rolled_back);
    assert!(
        report
            .rollback_steps
            .iter()
            .any(|step| step.step_id == "uninstall-service" && step.status == "failed")
    );
}

#[test]
fn failed_execution_report_maps_to_api_error() {
    let report = ServiceLifecycleExecutionReport {
        executed: true,
        dry_run: false,
        completed_steps: vec![ServiceLifecycleStepResult {
            step_id: "install-service".to_owned(),
            status: "failed".to_owned(),
            message: "forced failure".to_owned(),
        }],
        rollback_steps: Vec::new(),
        rolled_back: false,
        failed_step_id: Some("install-service".to_owned()),
    };

    let error = service_execution_error(&report).expect("failed report should map to error");

    assert!(error.message.contains("install-service"));
    assert!(error.message.contains("rollback_status=not_attempted"));
}

#[test]
fn explicit_rollback_uses_checkpoint_binary_path_without_install_dir_flag() {
    let root = unique_root("checkpoint-binary");
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let binary_path = root.join("bin").join("relay-knowledge");
    let binary_backup_path = binary_path.with_extension("rollback");
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "upgrade",
        "binary_path": binary_path,
        "definition_path": paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME),
        "checksum": "abc123",
        "definition_backup_path": paths.service_dir.join("relay-knowledge.service.rollback"),
        "binary_backup_path": binary_backup_path,
    });
    std::fs::write(
        paths
            .service_dir
            .join(SERVICE_LIFECYCLE_CHECKPOINT_FILE_NAME),
        serde_json::to_string(&checkpoint).expect("checkpoint should serialize"),
    )
    .expect("checkpoint should be written");
    let request = request(ServiceManagerAction::Rollback);

    let plan = render_service_plan_for_platform(
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
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_rollback_validates_checkpoint_before_stopping_service() {
    let root = unique_root("missing-checkpoint-before-stop");
    let paths = runtime_paths_at(&root);
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
    let step_ids: Vec<&str> = plan
        .lifecycle_steps
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    let mut runner = ProcessStepRunner;

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert!(
        step_ids
            .iter()
            .position(|id| *id == "validate-rollback-checkpoint")
            < step_ids.iter().position(|id| *id == "stop-service")
    );
    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("validate-rollback-checkpoint")
    );
    assert_eq!(report.completed_steps.len(), 1);
    assert!(
        report
            .completed_steps
            .iter()
            .any(|step| step.message.contains("read rollback checkpoint"))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_rollback_fails_when_checkpoint_backup_is_missing() {
    let root = unique_root("missing-backup");
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "upgrade",
        "binary_path": root.join("bin").join("relay-knowledge"),
        "definition_path": definition_path,
        "checksum": "abc123",
        "definition_backup_path": root.join("missing-definition.rollback"),
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

    assert_eq!(
        report.failed_step_id.as_deref(),
        Some("restore-service-definition")
    );
    assert!(!report.rolled_back);
    assert!(
        report
            .completed_steps
            .iter()
            .any(|step| step.message.contains("missing rollback backup"))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn capture_checkpoint_uses_distinct_backup_names_for_definition_and_binary() {
    let root = unique_root("distinct-checkpoint-backups");
    let paths = runtime_paths_at(&root);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    let definition_path = paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME);
    let binary_path = paths.service_dir.join(PROJECT_NAME);
    std::fs::write(&definition_path, b"old definition").expect("definition should be written");
    std::fs::write(&binary_path, b"old binary").expect("binary should be written");
    let mut request = request(ServiceManagerAction::Upgrade);
    request.dry_run = false;
    request.execute = true;
    request.install_dir = Some(paths.service_dir.display().to_string());
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
    plan.lifecycle_steps
        .retain(|step| step.id == "capture-rollback-checkpoint");
    let mut runner = ProcessStepRunner;

    let report = execute_service_plan_blocking(&plan, &mut runner);

    assert_eq!(report.failed_step_id, None);
    let checkpoint: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&plan.checkpoint_path).expect("checkpoint should be readable"),
    )
    .expect("checkpoint should parse");
    let definition_backup = PathBuf::from(
        checkpoint["definition_backup_path"]
            .as_str()
            .expect("definition backup should be recorded"),
    );
    let binary_backup = PathBuf::from(
        checkpoint["binary_backup_path"]
            .as_str()
            .expect("binary backup should be recorded"),
    );

    assert_ne!(definition_backup, binary_backup);
    assert!(
        definition_backup
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".definition.") && name.ends_with(".rollback"))
    );
    assert!(
        binary_backup
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains(".binary.") && name.ends_with(".rollback"))
    );
    assert_eq!(
        std::fs::read(&definition_backup).expect("definition backup should exist"),
        b"old definition"
    );
    assert_eq!(
        std::fs::read(&binary_backup).expect("binary backup should exist"),
        b"old binary"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn upgrade_rollback_removes_copied_binary_when_checkpoint_has_no_backup() {
    let root = unique_root("upgrade-no-binary-backup");
    let paths = runtime_paths_at(&root);
    let install_dir = root.join("bin");
    let binary_path = install_dir.join(PROJECT_NAME);
    std::fs::create_dir_all(&paths.service_dir).expect("service dir should be created");
    std::fs::create_dir_all(&install_dir).expect("install dir should be created");
    std::fs::write(&binary_path, b"new binary").expect("new binary should be written");
    let definition_backup = paths
        .service_dir
        .join(LINUX_SERVICE_DEFINITION_FILE_NAME)
        .with_extension("rollback");
    std::fs::write(&definition_backup, b"old definition")
        .expect("definition backup should be written");
    let checkpoint = serde_json::json!({
        "service_name": PROJECT_NAME,
        "action": "upgrade",
        "binary_path": binary_path,
        "definition_path": paths.service_dir.join(LINUX_SERVICE_DEFINITION_FILE_NAME),
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
    request.install_dir = Some(install_dir.display().to_string());
    let mut plan = render_service_plan_for_platform(
        &paths,
        StorageTopology::SingleSqlite,
        &request,
        "linux",
        PathBuf::from("/tmp/current/relay-knowledge"),
    )
    .expect("plan should render");
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
fn external_lifecycle_commands_time_out() {
    let command = vec!["sh".to_owned(), "-c".to_owned(), "sleep 1".to_owned()];

    let error = run_command_with_timeout(&command, Duration::from_millis(1))
        .expect_err("sleep should exceed the timeout");

    assert!(error.contains("timed out"));
}

fn runtime_paths() -> RuntimePaths {
    runtime_paths_at(Path::new("/tmp/relay-knowledge-lifecycle-tests/default"))
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
        "relay-knowledge-lifecycle-{name}-{}",
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

struct RollbackFailingRunner {
    lifecycle_fail_step: &'static str,
    rollback_fail_step: &'static str,
}

impl StepRunner for RollbackFailingRunner {
    fn run(
        &mut self,
        _plan: &ServiceDefinitionPlan,
        step: &ServiceLifecycleStep,
    ) -> Result<String, String> {
        if step.id == self.lifecycle_fail_step || step.id == self.rollback_fail_step {
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
