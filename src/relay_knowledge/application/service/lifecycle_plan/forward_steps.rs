//! Forward install, upgrade, and uninstall lifecycle step planning.

use crate::domain::ServiceLifecycleStep;

use super::{
    PlanContext,
    step_policy::{
        command_step, copy_binary_required, internal_step, relay_command,
        service_configuration_steps, service_registration_refresh_steps, service_reload_steps,
    },
};

pub(super) fn install_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
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

pub(super) fn upgrade_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
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

pub(super) fn uninstall_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
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

#[cfg(test)]
#[path = "forward_steps_tests.rs"]
mod tests;
