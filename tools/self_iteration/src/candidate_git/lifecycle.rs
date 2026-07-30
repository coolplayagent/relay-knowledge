use std::path::Path;

use crate::command::{CommandSpec, run_command};

use super::{
    PatchSnapshot,
    command::{git, git_checked},
    dynamic_command::git_dynamic,
};

pub fn reject_candidate(
    workspace: &Path,
    patch: &PatchSnapshot,
    hard_reset: bool,
) -> Result<(), String> {
    if hard_reset {
        git_checked(workspace, &["reset", "--hard", &patch.base_ref], 120)?;
        git_checked(workspace, &["clean", "-fd"], 120)?;
        return Ok(());
    }
    if patch.has_diff() {
        let result = run_command(&CommandSpec::new(
            "git_apply_reverse",
            vec![
                "git".to_owned(),
                "apply".to_owned(),
                "-R".to_owned(),
                patch.path.display().to_string(),
            ],
            workspace,
            None,
            120,
        ));
        if !result.passed() {
            return Err(result.gate_message());
        }
    }
    git_checked(workspace, &["reset", "--mixed", "HEAD"], 120)?;
    Ok(())
}

pub fn commit_candidate(
    workspace: &Path,
    commit_message: Option<&str>,
    score: f64,
    base_ref: &str,
) -> Result<String, String> {
    git_checked(workspace, &["reset", "--mixed", base_ref], 120)?;
    git_checked(workspace, &["add", "-A"], 120)?;
    let diff_status = git(workspace, &["diff", "--cached", "--quiet"], 120);
    if diff_status.exit_code == 0 {
        return Err("accepted candidate has no net diff to commit".to_owned());
    }
    if diff_status.exit_code != 1 {
        return Err(diff_status.gate_message());
    }
    let message = commit_message
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Self-iterate score {score:.6}"));
    git_dynamic(
        workspace,
        &["commit".to_owned(), "-m".to_owned(), message],
        120,
        true,
    )?;
    Ok(
        git_checked(workspace, &["rev-parse", "--short", "HEAD"], 60)?
            .stdout
            .trim()
            .to_owned(),
    )
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
