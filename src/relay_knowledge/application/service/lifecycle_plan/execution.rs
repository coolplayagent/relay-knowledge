use std::{
    collections::HashSet,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::domain::{
    ServiceDefinitionPlan, ServiceLifecycleExecutionReport, ServiceLifecycleStep,
    ServiceLifecycleStepResult, ServiceManagerAction,
};

const SERVICE_LIFECYCLE_COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const SERVICE_LIFECYCLE_COMMAND_OUTPUT_LIMIT: usize = 64 * 1024;
const SERVICE_LIFECYCLE_COMMAND_OUTPUT_JOIN_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn execute_service_plan_blocking(
    plan: &ServiceDefinitionPlan,
    runner: &mut dyn StepRunner,
) -> ServiceLifecycleExecutionReport {
    if plan.dry_run {
        return ServiceLifecycleExecutionReport {
            executed: false,
            dry_run: true,
            completed_steps: plan
                .lifecycle_steps
                .iter()
                .map(|step| step_result(&step.id, "dry_run", "not executed"))
                .collect(),
            rollback_steps: Vec::new(),
            rolled_back: false,
            failed_step_id: None,
        };
    }

    let mut completed_steps = Vec::new();
    let mut completed_step_ids = Vec::new();
    for step in &plan.lifecycle_steps {
        match runner.run(plan, step) {
            Ok(message) => {
                completed_step_ids.push(step.id.clone());
                completed_steps.push(step_result(&step.id, "completed", &message));
            }
            Err(message) => {
                let rollback_steps = if rollback_is_required(plan, &completed_step_ids, step) {
                    run_rollback_steps(plan, runner, &completed_step_ids, step)
                } else {
                    Vec::new()
                };
                let rolled_back = rollback_succeeded(&rollback_steps);
                completed_steps.push(step_result(&step.id, "failed", &message));
                return ServiceLifecycleExecutionReport {
                    executed: true,
                    dry_run: false,
                    completed_steps,
                    rollback_steps,
                    rolled_back,
                    failed_step_id: Some(step.id.clone()),
                };
            }
        }
    }

    ServiceLifecycleExecutionReport {
        executed: true,
        dry_run: false,
        completed_steps,
        rollback_steps: Vec::new(),
        rolled_back: false,
        failed_step_id: None,
    }
}

fn rollback_is_required(
    plan: &ServiceDefinitionPlan,
    completed_step_ids: &[String],
    failed_step: &ServiceLifecycleStep,
) -> bool {
    if plan.action == ServiceManagerAction::Rollback {
        return false;
    }
    if plan.action == ServiceManagerAction::Upgrade
        && failed_step.id == "capture-rollback-checkpoint"
    {
        return false;
    }
    if plan.action == ServiceManagerAction::Upgrade {
        return upgrade_rollback_is_required(completed_step_ids, failed_step);
    }
    if plan.action == ServiceManagerAction::Uninstall {
        return uninstall_rollback_is_required(completed_step_ids, failed_step);
    }
    completed_step_ids
        .iter()
        .any(|id| lifecycle_step_by_id(plan, id).is_some_and(step_can_mutate))
        || step_can_mutate(failed_step)
}

fn upgrade_rollback_is_required(
    completed_step_ids: &[String],
    failed_step: &ServiceLifecycleStep,
) -> bool {
    completed_step_ids.iter().any(|id| {
        matches!(
            id.as_str(),
            "copy-binary"
                | "write-service-definition"
                | "reload-service-manager"
                | "refresh-service-registration"
                | "unload-service-registration"
                | "load-service-registration"
                | "start-service"
        )
    }) || matches!(
        failed_step.id.as_str(),
        "copy-binary"
            | "write-service-definition"
            | "reload-service-manager"
            | "refresh-service-registration"
            | "unload-service-registration"
            | "load-service-registration"
            | "start-service"
            | "post-upgrade-doctor"
    )
}

fn uninstall_rollback_is_required(
    completed_step_ids: &[String],
    failed_step: &ServiceLifecycleStep,
) -> bool {
    completed_step_ids.iter().any(|id| {
        id == "stop-service" || id == "uninstall-service" || id == "reload-service-manager"
    }) || failed_step.id == "remove-service-definition"
}

fn lifecycle_step_by_id<'a>(
    plan: &'a ServiceDefinitionPlan,
    id: &str,
) -> Option<&'a ServiceLifecycleStep> {
    plan.lifecycle_steps.iter().find(|step| step.id == id)
}

fn step_can_mutate(step: &ServiceLifecycleStep) -> bool {
    !step.writes_paths.is_empty()
        || !step.removes_paths.is_empty()
        || matches!(
            step.id.as_str(),
            "capture-rollback-checkpoint"
                | "install-service"
                | "uninstall-service"
                | "start-service"
                | "stop-service"
                | "configure-service-environment"
                | "reload-service-manager"
                | "refresh-service-registration"
                | "unload-service-registration"
                | "load-service-registration"
                | "restore-service-definition"
                | "restore-binary"
        )
}

fn run_rollback_steps(
    plan: &ServiceDefinitionPlan,
    runner: &mut dyn StepRunner,
    completed_step_ids: &[String],
    failed_step: &ServiceLifecycleStep,
) -> Vec<ServiceLifecycleStepResult> {
    let mut results = Vec::new();
    let completed: HashSet<&str> = completed_step_ids.iter().map(String::as_str).collect();
    for step in plan
        .rollback_steps
        .iter()
        .filter(|rollback_step| rollback_step_applies(plan, &completed, failed_step, rollback_step))
    {
        match runner.run(plan, step) {
            Ok(message) => results.push(step_result(&step.id, "completed", &message)),
            Err(message) => {
                let stop_followups = rollback_failure_blocks_followups(step);
                results.push(step_result(&step.id, "failed", &message));
                if stop_followups {
                    break;
                }
            }
        }
    }
    results
}

fn rollback_failure_blocks_followups(step: &ServiceLifecycleStep) -> bool {
    matches!(
        step.id.as_str(),
        "restore-service-definition"
            | "restore-binary"
            | "uninstall-service"
            | "write-service-definition"
            | "install-service"
            | "configure-service-environment"
            | "refresh-service-registration"
            | "unload-service-registration"
            | "load-service-registration"
            | "reload-service-manager"
    )
}

fn rollback_step_applies(
    plan: &ServiceDefinitionPlan,
    completed: &HashSet<&str>,
    failed_step: &ServiceLifecycleStep,
    rollback_step: &ServiceLifecycleStep,
) -> bool {
    if plan.action == ServiceManagerAction::Uninstall {
        return uninstall_rollback_step_applies(completed, failed_step, rollback_step);
    }
    if plan.action != ServiceManagerAction::Install {
        return true;
    }

    let failed = failed_step.id.as_str();
    let binary_touched = completed.contains("copy-binary")
        || (failed == "copy-binary" && completed.contains("verify-install-target"));
    let definition_touched =
        completed.contains("write-service-definition") || failed == "write-service-definition";
    let manager_touched = completed.contains("install-service")
        || completed.contains("start-service")
        || failed == "start-service"
        || failed == "post-install-doctor";
    let reload_touched = completed.contains("reload-service-manager")
        || failed == "reload-service-manager"
        || definition_touched
        || manager_touched;

    match rollback_step.id.as_str() {
        "stop-service" | "uninstall-service" => manager_touched,
        "reload-service-manager" => reload_touched,
        "remove-service-definition" => definition_touched || manager_touched,
        "remove-installed-binary" => binary_touched || definition_touched || manager_touched,
        _ => true,
    }
}

fn uninstall_rollback_step_applies(
    completed: &HashSet<&str>,
    failed_step: &ServiceLifecycleStep,
    rollback_step: &ServiceLifecycleStep,
) -> bool {
    let failed = failed_step.id.as_str();
    let stop_completed = completed.contains("stop-service");
    let definition_removed = completed.contains("remove-service-definition");
    let manager_removed = completed.contains("uninstall-service")
        || completed.contains("reload-service-manager")
        || failed == "reload-service-manager"
        || failed == "remove-service-definition";

    match rollback_step.id.as_str() {
        "restore-service-definition" | "write-service-definition" => definition_removed,
        "install-service" => manager_removed,
        "configure-service-environment" => manager_removed,
        "start-service" | "post-install-doctor" => stop_completed || manager_removed,
        _ => false,
    }
}

fn rollback_succeeded(rollback_steps: &[ServiceLifecycleStepResult]) -> bool {
    !rollback_steps.is_empty() && rollback_steps.iter().all(|step| step.status == "completed")
}

pub(super) trait StepRunner {
    fn run(
        &mut self,
        plan: &ServiceDefinitionPlan,
        step: &ServiceLifecycleStep,
    ) -> Result<String, String>;
}

pub(super) struct ProcessStepRunner;

impl StepRunner for ProcessStepRunner {
    fn run(
        &mut self,
        plan: &ServiceDefinitionPlan,
        step: &ServiceLifecycleStep,
    ) -> Result<String, String> {
        match step.id.as_str() {
            "write-service-definition" => {
                write_file(Path::new(&plan.definition_path), plan.definition.as_bytes())?;
                Ok(format!("wrote {}", plan.definition_path))
            }
            "remove-service-definition" => {
                remove_file_if_exists(Path::new(&plan.definition_path))?;
                Ok(format!("removed {}", plan.definition_path))
            }
            "remove-installed-binary" => {
                remove_file_if_exists(Path::new(&plan.binary_path))?;
                Ok(format!("removed {}", plan.binary_path))
            }
            "capture-rollback-checkpoint" => {
                capture_checkpoint(plan)?;
                Ok(format!("wrote {}", plan.checkpoint_path))
            }
            "validate-rollback-checkpoint" => {
                validate_checkpoint(plan)?;
                Ok(format!("validated {}", plan.checkpoint_path))
            }
            "copy-binary" => {
                copy_current_binary(plan)?;
                Ok(format!("wrote {}", plan.binary_path))
            }
            "verify-install-target" => {
                verify_install_binary_target(plan)?;
                Ok(format!("verified {}", plan.binary_path))
            }
            "verify-service-definition-target" => {
                verify_service_definition_target(plan)?;
                Ok(format!("verified {}", plan.definition_path))
            }
            "restore-service-definition" => {
                let checkpoint = read_checkpoint(plan)?;
                restore_checkpoint_definition(&checkpoint)
            }
            "restore-binary" => {
                let checkpoint = read_checkpoint(plan)?;
                restore_checkpoint_binary(&checkpoint)
            }
            _ => run_command(&step.command),
        }
    }
}

pub(super) fn write_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, contents).map_err(|error| error.to_string())
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
struct LifecycleCheckpoint {
    service_name: String,
    action: String,
    binary_path: String,
    definition_path: String,
    checksum: String,
    definition_backup_path: Option<String>,
    binary_backup_path: Option<String>,
    #[serde(default)]
    binary_cleanup_on_no_backup: bool,
}

pub(super) fn checkpoint_binary_restore_path(checkpoint_path: &Path) -> Option<PathBuf> {
    read_checkpoint_from_path(checkpoint_path)
        .ok()
        .and_then(|checkpoint| {
            if checkpoint.binary_backup_path.is_some() || checkpoint.binary_cleanup_on_no_backup {
                Some(PathBuf::from(checkpoint.binary_path))
            } else {
                None
            }
        })
}

pub(super) fn checkpoint_action_is_uninstall(checkpoint_path: &Path) -> bool {
    read_checkpoint_from_path(checkpoint_path)
        .is_ok_and(|checkpoint| checkpoint.action == ServiceManagerAction::Uninstall.as_str())
}

fn capture_checkpoint(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let attempt_id = checkpoint_attempt_id();
    let definition_backup_path = backup_if_exists(
        Path::new(&plan.definition_path),
        CheckpointBackupKind::Definition,
        &attempt_id,
    )?;
    let binary_backup_path = if plan.install_dir.is_some() {
        backup_if_exists(
            Path::new(&plan.binary_path),
            CheckpointBackupKind::Binary,
            &attempt_id,
        )?
    } else {
        None
    };
    let binary_cleanup_on_no_backup = plan.install_dir.is_some() && binary_backup_path.is_none();
    let checkpoint = LifecycleCheckpoint {
        service_name: plan.service_name.clone(),
        action: plan.action.as_str().to_owned(),
        binary_path: plan.binary_path.clone(),
        definition_path: plan.definition_path.clone(),
        checksum: plan.checksum.clone(),
        definition_backup_path: definition_backup_path.map(|path| path.display().to_string()),
        binary_backup_path: binary_backup_path.map(|path| path.display().to_string()),
        binary_cleanup_on_no_backup,
    };
    write_checkpoint(Path::new(&plan.checkpoint_path), &checkpoint, &attempt_id)
}

fn write_checkpoint(
    path: &Path,
    checkpoint: &LifecycleCheckpoint,
    attempt_id: &str,
) -> Result<(), String> {
    let temporary_path = checkpoint_temporary_path(path, attempt_id);
    write_file(
        &temporary_path,
        serde_json::to_string_pretty(checkpoint)
            .map_err(|error| error.to_string())?
            .as_bytes(),
    )?;
    std::fs::rename(&temporary_path, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary_path);
        format!(
            "publish rollback checkpoint {} from {}: {error}",
            path.display(),
            temporary_path.display()
        )
    })
}

fn validate_checkpoint(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let checkpoint = read_checkpoint(plan)?;
    if checkpoint.service_name != plan.service_name {
        return Err(format!(
            "rollback checkpoint service {} does not match {}",
            checkpoint.service_name, plan.service_name
        ));
    }
    validate_checkpoint_definition_backup(&checkpoint)?;
    if let Some(binary_backup_path) = checkpoint.binary_backup_path.as_deref() {
        validate_checkpoint_backup(Some(binary_backup_path), "binary")?;
    }
    Ok(())
}

fn validate_checkpoint_definition_backup(checkpoint: &LifecycleCheckpoint) -> Result<(), String> {
    if let Some(backup_path) = checkpoint.definition_backup_path.as_deref() {
        validate_checkpoint_backup(Some(backup_path), "service definition")?;
        return Ok(());
    }
    if checkpoint.action == ServiceManagerAction::Upgrade.as_str() {
        return Ok(());
    }
    Err("rollback checkpoint does not contain service definition backup path".to_owned())
}

fn read_checkpoint(plan: &ServiceDefinitionPlan) -> Result<LifecycleCheckpoint, String> {
    read_checkpoint_from_path(Path::new(&plan.checkpoint_path))
}

fn read_checkpoint_from_path(path: &Path) -> Result<LifecycleCheckpoint, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("read rollback checkpoint {}: {error}", path.display()))?;
    serde_json::from_str(&contents)
        .map_err(|error| format!("parse rollback checkpoint {}: {error}", path.display()))
}

fn copy_current_binary(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let source = std::env::current_exe().map_err(|error| error.to_string())?;
    let target = Path::new(&plan.binary_path);
    if source == target {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn verify_install_binary_target(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let target = Path::new(&plan.binary_path);
    if target.exists() {
        return Err(format!(
            "install target binary already exists at {}; run service lifecycle upgrade --install-dir to replace it",
            target.display()
        ));
    }
    Ok(())
}

fn verify_service_definition_target(plan: &ServiceDefinitionPlan) -> Result<(), String> {
    let target = Path::new(&plan.definition_path);
    if target.exists() {
        return Err(format!(
            "service definition already exists at {}; run service lifecycle upgrade to replace it",
            target.display()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum CheckpointBackupKind {
    Definition,
    Binary,
}

impl CheckpointBackupKind {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Binary => "binary",
        }
    }
}

fn backup_if_exists(
    path: &Path,
    kind: CheckpointBackupKind,
    attempt_id: &str,
) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let backup = backup_path(path, kind, attempt_id);
    std::fs::copy(path, &backup)
        .map(|_| Some(backup))
        .map_err(|error| error.to_string())
}

fn restore_checkpoint_binary(checkpoint: &LifecycleCheckpoint) -> Result<String, String> {
    let binary_path = Path::new(&checkpoint.binary_path);
    if let Some(backup_path) = checkpoint.binary_backup_path.as_deref() {
        restore_checkpoint_backup(binary_path, Some(backup_path), "binary")?;
        return Ok(format!("restored {}", checkpoint.binary_path));
    }
    if checkpoint.binary_cleanup_on_no_backup {
        remove_file_if_exists(binary_path)?;
        return Ok(format!("removed {}", checkpoint.binary_path));
    }
    Err("rollback checkpoint does not contain binary backup path".to_owned())
}

fn restore_checkpoint_definition(checkpoint: &LifecycleCheckpoint) -> Result<String, String> {
    let definition_path = Path::new(&checkpoint.definition_path);
    if let Some(backup_path) = checkpoint.definition_backup_path.as_deref() {
        restore_checkpoint_backup(definition_path, Some(backup_path), "service definition")?;
        return Ok(format!("restored {}", checkpoint.definition_path));
    }
    if checkpoint.action == ServiceManagerAction::Upgrade.as_str() {
        remove_file_if_exists(definition_path)?;
        return Ok(format!("removed {}", checkpoint.definition_path));
    }
    Err("rollback checkpoint does not contain service definition backup path".to_owned())
}

fn restore_checkpoint_backup(
    path: &Path,
    backup_path: Option<&str>,
    label: &str,
) -> Result<(), String> {
    let backup = validate_checkpoint_backup(backup_path, label)?;
    std::fs::copy(&backup, path)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_checkpoint_backup(backup_path: Option<&str>, label: &str) -> Result<PathBuf, String> {
    let backup = backup_path
        .map(PathBuf::from)
        .ok_or_else(|| format!("rollback checkpoint does not contain {label} backup path"))?;
    if !backup.exists() {
        return Err(format!("missing rollback backup {}", backup.display()));
    }
    Ok(backup)
}

fn backup_path(path: &Path, kind: CheckpointBackupKind, attempt_id: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("checkpoint"));
    file_name.push(format!(".{}.{attempt_id}.rollback", kind.suffix()));
    path.with_file_name(file_name)
}

fn checkpoint_temporary_path(path: &Path, attempt_id: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("checkpoint"));
    file_name.push(format!(".{attempt_id}.tmp"));
    path.with_file_name(file_name)
}

fn checkpoint_attempt_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{timestamp}", std::process::id())
}

fn run_command(command: &[String]) -> Result<String, String> {
    run_command_with_timeout(command, SERVICE_LIFECYCLE_COMMAND_TIMEOUT)
}

pub(super) fn run_command_with_timeout(
    command: &[String],
    timeout: Duration,
) -> Result<String, String> {
    let Some(program) = command.first() else {
        return Ok("no external command".to_owned());
    };
    let mut child = Command::new(program)
        .args(&command[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .map(|pipe| drain_pipe_limited(pipe, SERVICE_LIFECYCLE_COMMAND_OUTPUT_LIMIT));
    let mut stderr = child
        .stderr
        .take()
        .map(|pipe| drain_pipe_limited(pipe, SERVICE_LIFECYCLE_COMMAND_OUTPUT_LIMIT));
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            let output = collect_child_output(
                stdout.take(),
                stderr.take(),
                SERVICE_LIFECYCLE_COMMAND_OUTPUT_JOIN_TIMEOUT,
            );
            if status.success() {
                return Ok(format!("exit_status={status}"));
            }
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            return Err(if detail.is_empty() {
                format!("exit_status={status}")
            } else {
                detail.to_owned()
            });
        }
        if Instant::now() >= deadline {
            terminate_child(&mut child);
            let _ = collect_child_output(
                stdout.take(),
                stderr.take(),
                SERVICE_LIFECYCLE_COMMAND_OUTPUT_JOIN_TIMEOUT,
            );
            return Err(format!("command timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

struct CommandOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn drain_pipe_limited<R>(mut pipe: R, limit: usize) -> JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut retained = Vec::new();
        let mut buffer = [0_u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => return retained,
                Ok(read) => {
                    let remaining = limit.saturating_sub(retained.len());
                    if remaining > 0 {
                        retained.extend_from_slice(&buffer[..read.min(remaining)]);
                    }
                }
                Err(_) => return retained,
            }
        }
    })
}

fn collect_child_output(
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
    timeout: Duration,
) -> CommandOutput {
    let deadline = Instant::now() + timeout;
    let stderr = join_output_until(stderr, deadline);
    let stdout = join_output_until(stdout, deadline);
    CommandOutput { stdout, stderr }
}

fn join_output_until(handle: Option<JoinHandle<Vec<u8>>>, deadline: Instant) -> Vec<u8> {
    let Some(handle) = handle else {
        return Vec::new();
    };
    while !handle.is_finished() {
        let now = Instant::now();
        if now >= deadline {
            return Vec::new();
        }
        thread::sleep((deadline - now).min(Duration::from_millis(10)));
    }
    handle.join().unwrap_or_default()
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn step_result(step_id: &str, status: &str, message: &str) -> ServiceLifecycleStepResult {
    ServiceLifecycleStepResult {
        step_id: step_id.to_owned(),
        status: status.to_owned(),
        message: message.to_owned(),
    }
}
