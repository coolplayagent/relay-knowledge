//! Service lifecycle plan assembly and request orchestration.

use std::path::{Component, Path, PathBuf};

use crate::{
    api::{ApiError, ServicePlanRequest},
    domain::{
        ServiceDefinitionPlan, ServiceLifecycleExecutionReport, ServiceManagerAction,
        ServicePackageManifestCheck,
    },
    paths::RuntimePaths,
    project::{PROJECT_NAME, SERVICE_LIFECYCLE_CHECKPOINT_FILE_NAME},
    storage::StorageTopology,
};

use super::RelayKnowledgeService;

mod checkpoint;
mod execution;
mod forward_steps;
mod platform_service;
mod process_runner;
mod rollback_steps;
mod step_policy;

#[cfg(test)]
use crate::project::{
    LINUX_SERVICE_DEFINITION_FILE_NAME, MACOS_SERVICE_DEFINITION_FILE_NAME,
    WINDOWS_SERVICE_DEFINITION_FILE_NAME,
};
use checkpoint::write_file;
use execution::{ProcessStepRunner, execute_service_plan_blocking};
use forward_steps::{install_steps, uninstall_steps, upgrade_steps};
use platform_service::{
    binary_path, current_platform, install_command, permission_requirements, render_definition,
    service_definition_filename, start_command, stop_command, uninstall_command,
};
use rollback_steps::rollback_steps;

#[cfg(test)]
use execution::StepRunner;
#[cfg(all(test, unix))]
use process_runner::run_command_with_timeout;

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
    let lifecycle_steps = match request.action {
        ServiceManagerAction::Install => install_steps(&context),
        ServiceManagerAction::Upgrade => upgrade_steps(&context),
        ServiceManagerAction::Rollback => rollback_steps(request.action, &context),
        ServiceManagerAction::Uninstall => uninstall_steps(&context),
    };
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
#[path = "mod_tests.rs"]
mod tests;
