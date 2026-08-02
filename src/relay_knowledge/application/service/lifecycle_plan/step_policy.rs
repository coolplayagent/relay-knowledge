//! Shared lifecycle step construction and platform registration policies.

use std::path::Path;

use crate::domain::ServiceLifecycleStep;

use super::{
    PlanContext,
    platform_service::{
        windows_configure_environment_command, windows_refresh_registration_command,
    },
};

pub(super) fn service_reload_steps(platform: &str) -> Vec<ServiceLifecycleStep> {
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

pub(super) fn service_configuration_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
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

pub(super) fn service_registration_refresh_steps(
    context: &PlanContext<'_>,
) -> Vec<ServiceLifecycleStep> {
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

pub(super) fn command_step(
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

pub(super) fn internal_step(
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

pub(super) fn relay_command<const N: usize>(binary_path: &Path, args: [&str; N]) -> Vec<String> {
    let mut command = vec![binary_path.display().to_string()];
    command.extend(args.into_iter().map(str::to_owned));
    command
}

pub(super) fn copy_binary_required(context: &PlanContext<'_>) -> bool {
    context.install_dir.is_some() && context.source_binary_path != context.binary_path
}

#[cfg(test)]
#[path = "step_policy_tests.rs"]
mod tests;
