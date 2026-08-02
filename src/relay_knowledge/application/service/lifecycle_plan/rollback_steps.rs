//! Rollback and checkpoint-restoration lifecycle step planning.

use crate::domain::{ServiceLifecycleStep, ServiceManagerAction};

use super::{
    PlanContext, checkpoint,
    step_policy::{
        command_step, copy_binary_required, internal_step, service_configuration_steps,
        service_registration_refresh_steps, service_reload_steps,
    },
};

pub(super) fn rollback_steps(
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
        || checkpoint::checkpoint_binary_restore_path(context.checkpoint_path).is_some()
}

fn explicit_checkpoint_rollback_steps(context: &PlanContext<'_>) -> Vec<ServiceLifecycleStep> {
    explicit_rollback_steps(
        context,
        rollback_should_restore_binary(context),
        true,
        checkpoint::checkpoint_action_is_uninstall(context.checkpoint_path),
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
            super::step_policy::relay_command(
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
    let checkpoint_binary_path =
        checkpoint::checkpoint_binary_restore_path(context.checkpoint_path);
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

#[cfg(test)]
#[path = "rollback_steps_tests.rs"]
mod tests;
