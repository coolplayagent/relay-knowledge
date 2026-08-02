//! Lifecycle step execution, rollback admission, and runner dispatch.

use std::{collections::HashSet, path::Path};

use crate::domain::{
    ServiceDefinitionPlan, ServiceLifecycleExecutionReport, ServiceLifecycleStep,
    ServiceLifecycleStepResult, ServiceManagerAction,
};

use super::{
    checkpoint::{
        capture_checkpoint, copy_current_binary, remove_file_if_exists, restore_checkpoint_binary,
        restore_checkpoint_definition, validate_checkpoint, verify_install_binary_target,
        verify_service_definition_target, write_file,
    },
    process_runner::run_command,
};

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
            "restore-service-definition" => restore_checkpoint_definition(plan),
            "restore-binary" => restore_checkpoint_binary(plan),
            _ => run_command(&step.command),
        }
    }
}

fn step_result(step_id: &str, status: &str, message: &str) -> ServiceLifecycleStepResult {
    ServiceLifecycleStepResult {
        step_id: step_id.to_owned(),
        status: status.to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
