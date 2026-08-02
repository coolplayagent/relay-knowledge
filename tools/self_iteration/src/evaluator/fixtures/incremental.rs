use std::{
    fs,
    path::{Component, Path},
    time::Instant,
};

use serde_json::Value;

use crate::{
    cases::{array_field, string_field},
    command::{CommandResult, CommandSpec},
};

use super::{repository::generated_git_env, write_fixture_file};
use crate::evaluator::runtime::{concurrency::run_limited, contracts::EvalRuntime};

pub(in crate::evaluator) struct IncrementalCommit {
    pub(in crate::evaluator) base_ref: String,
    pub(in crate::evaluator) head_ref: String,
    pub(in crate::evaluator) changed_path_count: usize,
    pub(in crate::evaluator) commands: Vec<CommandResult>,
}

pub(in crate::evaluator) fn prepare_incremental_repository_change(
    runtime: &EvalRuntime,
    repo_name: &str,
    root: &Path,
    repo_config: &Value,
) -> Result<Option<IncrementalCommit>, String> {
    let changes = array_field(repo_config, "incremental_changes");
    if changes.is_empty() {
        return Ok(None);
    }
    let env = generated_git_env(&runtime.env);
    let mut commands = Vec::new();
    let base = run_git(
        runtime,
        repo_name,
        root,
        &env,
        "incremental_base",
        ["rev-parse", "HEAD"],
    );
    let base_ref = base.stdout.trim().to_owned();
    let base_passed = base.passed() && !base_ref.is_empty();
    commands.push(base);
    if !base_passed {
        return Ok(Some(IncrementalCommit {
            base_ref,
            head_ref: String::new(),
            changed_path_count: changes.len(),
            commands,
        }));
    }

    let mutation = apply_incremental_changes(repo_name, root, changes);
    let mutation_passed = mutation.passed();
    commands.push(mutation);
    if !mutation_passed {
        return Ok(Some(IncrementalCommit {
            base_ref,
            head_ref: String::new(),
            changed_path_count: changes.len(),
            commands,
        }));
    }
    commands.push(run_git(
        runtime,
        repo_name,
        root,
        &env,
        "incremental_add",
        ["add", "-A"],
    ));
    if commands.last().is_some_and(|command| !command.passed()) {
        return Ok(Some(IncrementalCommit {
            base_ref,
            head_ref: String::new(),
            changed_path_count: changes.len(),
            commands,
        }));
    }
    commands.push(run_git(
        runtime,
        repo_name,
        root,
        &env,
        "incremental_commit",
        [
            "commit",
            "--no-gpg-sign",
            "-q",
            "-m",
            "Apply incremental performance fixture change",
        ],
    ));
    if commands.last().is_some_and(|command| !command.passed()) {
        return Ok(Some(IncrementalCommit {
            base_ref,
            head_ref: String::new(),
            changed_path_count: changes.len(),
            commands,
        }));
    }
    let head = run_git(
        runtime,
        repo_name,
        root,
        &env,
        "incremental_head",
        ["rev-parse", "HEAD"],
    );
    let head_ref = head.stdout.trim().to_owned();
    commands.push(head);

    Ok(Some(IncrementalCommit {
        base_ref,
        head_ref,
        changed_path_count: changes.len(),
        commands,
    }))
}

fn apply_incremental_changes(repo_name: &str, root: &Path, changes: &[Value]) -> CommandResult {
    let started = Instant::now();
    let result = changes.iter().try_for_each(|change| {
        let path = string_field(change, "path")
            .ok_or_else(|| "incremental change requires a path".to_owned())?;
        let relative = safe_incremental_path(path)?;
        let target = root.join(relative);
        if change
            .get("delete")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return fs::remove_file(&target)
                .map_err(|error| format!("failed to delete {}: {error}", target.display()));
        }
        if fs::symlink_metadata(&target).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!(
                "incremental change refuses symlink target: {}",
                target.display()
            ));
        }
        let content = string_field(change, "content")
            .ok_or_else(|| format!("incremental change {path:?} requires content"))?;
        write_fixture_file(&target, content)
    });
    let (exit_code, stderr) = match result {
        Ok(()) => (0, String::new()),
        Err(error) => (1, error),
    };
    CommandResult {
        name: format!("{repo_name}_incremental_fixture_mutation"),
        command: vec![
            "prepare".to_owned(),
            "incremental-fixture".to_owned(),
            changes.len().to_string(),
        ],
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        stdout: String::new(),
        stderr,
    }
}

fn safe_incremental_path(path: &str) -> Result<&Path, String> {
    if path.is_empty() || path.contains(['\\', '\0', '\n', '\r']) {
        return Err(format!("unsafe incremental fixture path: {path:?}"));
    }
    let relative = Path::new(path);
    if !relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!("unsafe incremental fixture path: {path:?}"));
    }
    Ok(relative)
}

fn run_git<const N: usize>(
    runtime: &EvalRuntime,
    repo_name: &str,
    root: &Path,
    env: &std::collections::BTreeMap<String, String>,
    step: &str,
    args: [&str; N],
) -> CommandResult {
    let mut command = vec!["git".to_owned()];
    command.extend(args.into_iter().map(ToOwned::to_owned));
    run_limited(
        &runtime.limiter,
        CommandSpec::new(
            format!("{repo_name}_{step}"),
            command,
            root,
            Some(env.clone()),
            runtime.timeout.min(30),
        ),
    )
}

#[cfg(test)]
#[path = "incremental_tests.rs"]
mod incremental_tests;
