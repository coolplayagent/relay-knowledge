use std::path::{Component, Path, PathBuf};

use crate::{
    api::{ApiError, ServicePlanRequest},
    domain::{
        ServiceDefinitionPlan, ServiceLifecycleExecutionReport, ServiceLifecycleStep,
        ServiceManagerAction, ServicePackageManifestCheck,
    },
    paths::RuntimePaths,
    project::{PROJECT_NAME, SERVICE_LIFECYCLE_CHECKPOINT_FILE_NAME},
    storage::StorageTopology,
};

use super::RelayKnowledgeService;

mod execution;
mod platform_service;

#[cfg(test)]
use crate::project::{
    LINUX_SERVICE_DEFINITION_FILE_NAME, MACOS_SERVICE_DEFINITION_FILE_NAME,
    WINDOWS_SERVICE_DEFINITION_FILE_NAME,
};
use execution::{ProcessStepRunner, execute_service_plan_blocking, write_file};
use platform_service::{
    binary_path, current_platform, install_command, permission_requirements, render_definition,
    service_definition_filename, start_command, stop_command, uninstall_command,
    windows_configure_environment_command, windows_refresh_registration_command,
};

#[cfg(test)]
use execution::{StepRunner, run_command_with_timeout};

impl RelayKnowledgeService {
    pub(crate) fn render_service_plan_for_request(
        &self,
        request: &ServicePlanRequest,
    ) -> Result<ServiceDefinitionPlan, String> {
        if request.execute && request.dry_run {
            return Err("--execute cannot be combined with --dry-run".to_owned());
        }
        if !request.execute && !request.dry_run {
            return Err("service lifecycle requests must choose dry-run or --execute".to_owned());
        }
        let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(PROJECT_NAME));
        render_service_plan_for_platform(
            &self.runtime.paths,
            self.runtime.storage.topology,
            request,
            current_platform(),
            current_exe,
        )
    }

    pub(crate) async fn write_service_definition_from_plan(
        &self,
        plan: &ServiceDefinitionPlan,
    ) -> Result<(), ApiError> {
        let path = PathBuf::from(&plan.definition_path);
        let contents = plan.definition.clone();
        tokio::task::spawn_blocking(move || write_file(&path, contents.as_bytes()))
            .await
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))?
            .map_err(|error| ApiError::storage_unavailable(error.to_string()))
    }

    pub(crate) async fn execute_service_plan(
        &self,
        plan: &ServiceDefinitionPlan,
    ) -> Result<ServiceLifecycleExecutionReport, ApiError> {
        let plan = plan.clone();
        let report = tokio::task::spawn_blocking(move || {
            let mut runner = ProcessStepRunner;
            execute_service_plan_blocking(&plan, &mut runner)
        })
        .await
        .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
        if let Some(error) = service_execution_error(&report) {
            return Err(error);
        }
        Ok(report)
    }
}

fn service_execution_error(report: &ServiceLifecycleExecutionReport) -> Option<ApiError> {
    let failed_step_id = report.failed_step_id.as_deref()?;
    let rollback_status = if report.rollback_steps.is_empty() {
        "not_attempted"
    } else if report.rolled_back {
        "completed"
    } else {
        "failed"
    };
    Some(ApiError::storage_unavailable(format!(
        "service lifecycle execution failed at step {failed_step_id}; rollback_status={rollback_status}; completed_steps={}; rollback_steps={}",
        report.completed_steps.len(),
        report.rollback_steps.len()
    )))
}

fn render_service_plan_for_platform(
    paths: &RuntimePaths,
    topology: StorageTopology,
    request: &ServicePlanRequest,
    platform: &str,
    current_exe: PathBuf,
) -> Result<ServiceDefinitionPlan, String> {
    let target_version = normalized_target_version(request.target_version.as_deref())?;
    let install_dir = normalized_install_dir(request.install_dir.as_deref())?;
    let binary_path = binary_path(platform, install_dir.as_deref(), &current_exe);
    let definition_path = paths
        .service_dir
        .join(service_definition_filename(platform));
    let checkpoint_path = paths
        .service_dir
        .join(SERVICE_LIFECYCLE_CHECKPOINT_FILE_NAME);
    let definition = render_definition(
        platform,
        &binary_path.display().to_string(),
        &paths.data_dir.display().to_string(),
    );
    let checksum = format!("{:016x}", stable_hash64(definition.as_bytes()));
    let mut runtime_state_paths = vec![
        paths.database_file().display().to_string(),
        paths.config_dir.display().to_string(),
        paths.state_dir.display().to_string(),
        paths.log_dir.display().to_string(),
        paths.cache_dir.display().to_string(),
    ];
    let mut warnings = vec![
        "dry-run is the default; pass --execute to run local file steps and platform service-manager commands".to_owned(),
        "runtime state is preserved unless an operator explicitly removes it after reviewing runtime_state_paths".to_owned(),
    ];
    if topology == StorageTopology::PartitionedSqlite {
        runtime_state_paths.push(paths.repository_shards_dir().display().to_string());
        warnings.push(
            "partitioned_sqlite backup, migration, rollback, and uninstall confirmation must include both the control database and repository shard directory"
                .to_owned(),
        );
    }
    if request.action == ServiceManagerAction::Rollback {
        warnings.push(
            "rollback restores checkpointed binary and service definition files when the lifecycle checkpoint exists; data migrations still require their own checkpoint policy"
                .to_owned(),
        );
    }

    let install_command = install_command(platform, &definition_path, &binary_path);
    let uninstall_command = uninstall_command(platform, &definition_path);
    let start_command = start_command(platform);
    let stop_command = stop_command(platform);
    let context = PlanContext {
        platform,
        definition_path: &definition_path,
        binary_path: &binary_path,
        source_binary_path: &current_exe,
        checkpoint_path: &checkpoint_path,
        install_dir: install_dir.as_deref(),
        install_command: &install_command,
        uninstall_command: &uninstall_command,
        start_command: &start_command,
        stop_command: &stop_command,
    };
    let package_manifest_checks = package_manifest_checks(target_version.as_deref());
    let lifecycle_steps = lifecycle_steps(request.action, &context);
    let rollback_steps = rollback_steps(request.action, &context);

    Ok(ServiceDefinitionPlan {
        action: request.action,
        dry_run: request.dry_run,
        platform: platform.to_owned(),
        service_name: PROJECT_NAME.to_owned(),
        target_version,
        install_dir: install_dir.as_ref().map(|path| path.display().to_string()),
        binary_path: binary_path.display().to_string(),
        definition_path: definition_path.display().to_string(),
        install_command,
        uninstall_command,
        start_command,
        stop_command,
        lifecycle_steps,
        rollback_steps,
        permission_requirements: permission_requirements(platform),
        package_manifest_checks,
        runtime_state_paths,
        checkpoint_path: checkpoint_path.display().to_string(),
        warnings,
        definition,
        checksum,
    })
}

struct PlanContext<'a> {
    platform: &'a str,
    definition_path: &'a Path,
    binary_path: &'a Path,
    source_binary_path: &'a Path,
    checkpoint_path: &'a Path,
    install_dir: Option<&'a Path>,
    install_command: &'a [String],
    uninstall_command: &'a [String],
    start_command: &'a [String],
    stop_command: &'a [String],
}

fn lifecycle_steps(
    action: ServiceManagerAction,
    context: &PlanContext<'_>,
) -> Vec<ServiceLifecycleStep> {
    match action {
        ServiceManagerAction::Install => install_steps(context),
        ServiceManagerAction::Upgrade => upgrade_steps(context),
        ServiceManagerAction::Rollback => explicit_checkpoint_rollback_steps(context),
        ServiceManagerAction::Uninstall => uninstall_steps(context),
    }
}

fn install_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    let mut steps = vec![command_step(
        "preflight-doctor",
        "preflight",
        "Run setup diagnostics before writing service files.",
        relay_command(
            context.source_binary_path,
            ["setup", "doctor", "--format", "json"],
        ),
        false,
    )];
    steps.push(internal_step(
        "verify-service-definition-target",
        "preflight",
        "Verify a fresh install will not overwrite an existing service definition.",
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ));
    if copy_binary_required(context) {
        steps.push(internal_step(
            "verify-install-target",
            "preflight",
            "Verify the selected install directory will not overwrite an existing binary.",
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ));
        steps.push(internal_step(
            "copy-binary",
            "install",
            "Copy the current binary into the selected install directory.",
            Vec::new(),
            vec![context.binary_path],
            Vec::new(),
        ));
    }
    steps.push(internal_step(
        "write-service-definition",
        "install",
        "Write the platform service definition file.",
        relay_command(
            context.binary_path,
            ["service", "definition", "write", "--format", "json"],
        ),
        vec![context.definition_path],
        Vec::new(),
    ));
    steps.extend(service_reload_steps(context.platform));
    steps.push(command_step(
        "install-service",
        "install",
        "Register the service with the platform service manager.",
        context.install_command.to_vec(),
        true,
    ));
    steps.extend(service_configuration_steps(context));
    steps.extend([
        command_step(
            "start-service",
            "install",
            "Start the service through the platform service manager.",
            context.start_command.to_vec(),
            true,
        ),
        command_step(
            "post-install-doctor",
            "verify",
            "Run service diagnostics after installation.",
            relay_command(
                context.binary_path,
                ["service", "doctor", "--format", "json"],
            ),
            false,
        ),
    ]);
    steps
}

fn upgrade_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    let mut steps = vec![
        command_step(
            "preflight-doctor",
            "preflight",
            "Run setup diagnostics before changing the installed service.",
            relay_command(
                context.source_binary_path,
                ["setup", "doctor", "--format", "json"],
            ),
            false,
        ),
        internal_step(
            "capture-rollback-checkpoint",
            "checkpoint",
            "Record rollback metadata and backup existing definition and installed binary when present.",
            Vec::new(),
            vec![context.checkpoint_path],
            Vec::new(),
        ),
        command_step(
            "stop-service",
            "upgrade",
            "Stop the service before replacing service files.",
            context.stop_command.to_vec(),
            true,
        ),
    ];
    if copy_binary_required(context) {
        steps.push(internal_step(
            "copy-binary",
            "upgrade",
            "Copy the current binary into the selected install directory.",
            Vec::new(),
            vec![context.binary_path],
            Vec::new(),
        ));
    }
    steps.push(internal_step(
        "write-service-definition",
        "upgrade",
        "Write the upgraded platform service definition file.",
        relay_command(
            context.binary_path,
            ["service", "definition", "write", "--format", "json"],
        ),
        vec![context.definition_path],
        Vec::new(),
    ));
    steps.extend(service_registration_refresh_steps(context));
    steps.extend([
        command_step(
            "start-service",
            "upgrade",
            "Start the upgraded service through the platform service manager.",
            context.start_command.to_vec(),
            true,
        ),
        command_step(
            "post-upgrade-doctor",
            "verify",
            "Run service diagnostics after upgrade.",
            relay_command(
                context.binary_path,
                ["service", "doctor", "--format", "json"],
            ),
            false,
        ),
    ]);
    steps
}

fn uninstall_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    let mut steps = vec![
        internal_step(
            "capture-rollback-checkpoint",
            "preflight",
            "Record rollback metadata and backup the service definition before uninstall.",
            Vec::new(),
            vec![context.checkpoint_path],
            Vec::new(),
        ),
        command_step(
            "stop-service",
            "uninstall",
            "Stop the service before uninstalling the service manager registration.",
            context.stop_command.to_vec(),
            true,
        ),
        command_step(
            "uninstall-service",
            "uninstall",
            "Remove the service manager registration.",
            context.uninstall_command.to_vec(),
            true,
        ),
    ];
    steps.push(internal_step(
        "remove-service-definition",
        "uninstall",
        "Remove the generated service definition file while preserving runtime state paths.",
        Vec::new(),
        Vec::new(),
        vec![context.definition_path],
    ));
    steps.extend(service_reload_steps(context.platform));
    steps
}

fn rollback_steps(
    action: ServiceManagerAction,
    context: &PlanContext<'_>,
) -> Vec<ServiceLifecycleStep> {
    match action {
        ServiceManagerAction::Install => install_rollback_steps(context),
        ServiceManagerAction::Upgrade => {
            explicit_rollback_steps(context, context.install_dir.is_some(), false, false)
        }
        ServiceManagerAction::Rollback => explicit_checkpoint_rollback_steps(context),
        ServiceManagerAction::Uninstall => uninstall_rollback_steps(context),
    }
}

fn rollback_should_restore_binary(context: &PlanContext<'_>) -> bool {
    context.install_dir.is_some()
        || execution::checkpoint_binary_restore_path(context.checkpoint_path).is_some()
}

fn explicit_checkpoint_rollback_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    explicit_rollback_steps(
        context,
        rollback_should_restore_binary(context),
        true,
        execution::checkpoint_action_is_uninstall(context.checkpoint_path),
    )
}

fn install_rollback_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    let mut steps = vec![
        command_step(
            "stop-service",
            "rollback",
            "Stop a service instance that was started by the failed install attempt.",
            context.stop_command.to_vec(),
            true,
        ),
        command_step(
            "uninstall-service",
            "rollback",
            "Remove service-manager registration created by the failed install attempt.",
            context.uninstall_command.to_vec(),
            true,
        ),
    ];
    steps.push(internal_step(
        "remove-service-definition",
        "rollback",
        "Remove the service definition written by the failed install attempt.",
        Vec::new(),
        Vec::new(),
        vec![context.definition_path],
    ));
    steps.extend(service_reload_steps(context.platform));
    if copy_binary_required(context) {
        steps.push(internal_step(
            "remove-installed-binary",
            "rollback",
            "Remove the binary copied by the failed install attempt.",
            Vec::new(),
            Vec::new(),
            vec![context.binary_path],
        ));
    }
    steps
}

fn uninstall_rollback_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    let mut steps = vec![
        internal_step(
            "restore-service-definition",
            "rollback",
            "Restore the service definition removed by the failed uninstall attempt.",
            Vec::new(),
            vec![context.definition_path],
            Vec::new(),
        ),
        command_step(
            "install-service",
            "rollback",
            "Restore service-manager registration removed by the failed uninstall attempt.",
            context.install_command.to_vec(),
            true,
        ),
    ];
    steps.extend(service_configuration_steps(context));
    steps.extend([
        command_step(
            "start-service",
            "rollback",
            "Restart the service after a failed uninstall attempt stopped it.",
            context.start_command.to_vec(),
            true,
        ),
        command_step(
            "post-install-doctor",
            "verify",
            "Run service diagnostics after uninstall rollback.",
            relay_command(
                context.binary_path,
                ["service", "doctor", "--format", "json"],
            ),
            false,
        ),
    ]);
    steps
}

fn explicit_rollback_steps(
    context: &PlanContext<'_>,
    restore_binary_from_checkpoint: bool,
    validate_checkpoint_first: bool,
    restore_uninstall_registration: bool,
) -> Vec<ServiceLifecycleStep> {
    let mut steps = Vec::new();
    if validate_checkpoint_first {
        steps.push(internal_step(
            "validate-rollback-checkpoint",
            "preflight",
            "Validate the lifecycle checkpoint and backup files before stopping the live service.",
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ));
    }
    if restore_uninstall_registration {
        steps.extend(uninstall_rollback_steps(context));
        return steps;
    }
    steps.extend([
        command_step(
            "stop-service",
            "rollback",
            "Stop the service before restoring checkpointed files.",
            context.stop_command.to_vec(),
            true,
        ),
        internal_step(
            "restore-service-definition",
            "rollback",
            "Restore or remove the service definition according to the lifecycle checkpoint.",
            Vec::new(),
            vec![context.definition_path],
            Vec::new(),
        ),
    ]);
    let checkpoint_binary_path = execution::checkpoint_binary_restore_path(context.checkpoint_path);
    let rollback_binary_path = checkpoint_binary_path
        .as_deref()
        .or_else(|| context.install_dir.map(|_| context.binary_path));
    if restore_binary_from_checkpoint {
        let binary_path = rollback_binary_path.unwrap_or(context.binary_path);
        steps.push(internal_step(
            "restore-binary",
            "rollback",
            "Restore or remove the installed binary according to the lifecycle checkpoint.",
            Vec::new(),
            vec![binary_path],
            Vec::new(),
        ));
    }
    steps.extend(service_registration_refresh_steps(context));
    steps.push(command_step(
        "start-service",
        "rollback",
        "Start the restored service through the platform service manager.",
        context.start_command.to_vec(),
        true,
    ));
    steps
}

fn service_reload_steps(platform: &str) -> Vec<ServiceLifecycleStep> {
    match platform {
        "linux" => vec![command_step(
            "reload-service-manager",
            "service-manager",
            "Reload the user systemd manager after service definition changes.",
            vec![
                "systemctl".to_owned(),
                "--user".to_owned(),
                "daemon-reload".to_owned(),
            ],
            false,
        )],
        _ => Vec::new(),
    }
}

fn service_configuration_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    match context.platform {
        "windows" => vec![command_step(
            "configure-service-environment",
            "service-manager",
            "Write Windows Service environment settings after service creation.",
            windows_configure_environment_command(context.definition_path),
            true,
        )],
        _ => Vec::new(),
    }
}

fn service_registration_refresh_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    match context.platform {
        "windows" => vec![command_step(
            "refresh-service-registration",
            "service-manager",
            "Update the Windows Service command line and environment before restart.",
            windows_refresh_registration_command(context.definition_path),
            true,
        )],
        "macos" => vec![
            command_step(
                "unload-service-registration",
                "service-manager",
                "Unload the previous launchd job before loading the updated plist.",
                context.uninstall_command.to_vec(),
                true,
            ),
            command_step(
                "load-service-registration",
                "service-manager",
                "Load the updated launchd plist before restart.",
                context.install_command.to_vec(),
                true,
            ),
        ],
        _ => service_reload_steps(context.platform),
    }
}

fn command_step(
    id: &str,
    phase: &str,
    description: &str,
    command: Vec<String>,
    requires_privilege: bool,
) -> ServiceLifecycleStep {
    ServiceLifecycleStep {
        id: id.to_owned(),
        phase: phase.to_owned(),
        description: description.to_owned(),
        command,
        writes_paths: Vec::new(),
        removes_paths: Vec::new(),
        requires_privilege,
    }
}

fn internal_step(
    id: &str,
    phase: &str,
    description: &str,
    command: Vec<String>,
    writes_paths: Vec<&Path>,
    removes_paths: Vec<&Path>,
) -> ServiceLifecycleStep {
    ServiceLifecycleStep {
        id: id.to_owned(),
        phase: phase.to_owned(),
        description: description.to_owned(),
        command,
        writes_paths: writes_paths
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
        removes_paths: removes_paths
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
        requires_privilege: false,
    }
}

fn relay_command<const N: usize>(binary_path: &Path, args: [&str; N]) -> Vec<String> {
    let mut command = vec![binary_path.display().to_string()];
    command.extend(args.into_iter().map(str::to_owned));
    command
}

fn copy_binary_required(context: &PlanContext<'_>) -> bool {
    context.install_dir.is_some() && context.source_binary_path != context.binary_path
}

fn package_manifest_checks(target_version: Option<&str>) -> Vec<ServicePackageManifestCheck> {
    let tag = target_version.unwrap_or(env!("CARGO_PKG_VERSION"));
    ["homebrew", "scoop", "winget", "distro"]
        .into_iter()
        .map(|manager| ServicePackageManifestCheck {
            manager: manager.to_owned(),
            artifact_source: format!("GitHub Release tag {tag}"),
            verification: "manifest artifacts and checksums must reference the same release tag as the packaged binary".to_owned(),
        })
        .collect()
}

fn normalized_target_version(value: Option<&str>) -> Result<Option<String>, String> {
    value
        .map(|version| {
            let trimmed = version.trim();
            if trimmed.is_empty() {
                Err("--target-version must not be empty".to_owned())
            } else {
                Ok(trimmed.to_owned())
            }
        })
        .transpose()
}

fn normalized_install_dir(value: Option<&str>) -> Result<Option<PathBuf>, String> {
    value
        .map(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Err("--install-dir must not be empty".to_owned());
            }
            let path = PathBuf::from(trimmed);
            if !path.is_absolute() {
                return Err("--install-dir must be an absolute path".to_owned());
            }
            if path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            {
                return Err("--install-dir must not contain '..'".to_owned());
            }
            Ok(path)
        })
        .transpose()
}

fn stable_hash64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
#[path = "lifecycle_plan_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lifecycle_plan_review_tests.rs"]
mod review_tests;

#[cfg(test)]
#[path = "lifecycle_plan_review_followup_tests.rs"]
mod review_followup_tests;
