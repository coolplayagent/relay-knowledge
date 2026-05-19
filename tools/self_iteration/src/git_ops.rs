use std::{path::Path, time::Duration};

use sha2::{Digest, Sha256};

use crate::{
    command::{CommandResult, CommandSpec, run_command},
    history::HistoryPaths,
};

#[derive(Debug, Clone)]
pub struct PatchSnapshot {
    pub path: std::path::PathBuf,
    pub diff: String,
    pub sha256: String,
    pub base_ref: String,
}

impl PatchSnapshot {
    pub fn has_diff(&self) -> bool {
        !self.diff.trim().is_empty()
    }
}

pub fn git(workspace: &Path, args: &[&str], timeout_seconds: u64) -> CommandResult {
    let mut command = vec!["git".to_owned()];
    command.extend(args.iter().map(|arg| (*arg).to_owned()));
    run_command(&CommandSpec::new(
        "git",
        command,
        workspace,
        None,
        timeout_seconds,
    ))
}

pub fn git_checked(
    workspace: &Path,
    args: &[&str],
    timeout_seconds: u64,
) -> Result<CommandResult, String> {
    let result = git(workspace, args, timeout_seconds);
    if result.passed() {
        Ok(result)
    } else {
        Err(result.gate_message())
    }
}

pub fn ensure_clean_worktree(workspace: &Path) -> Result<(), String> {
    let result = git_checked(workspace, &["status", "--porcelain"], 60)?;
    if result.stdout.trim().is_empty() {
        Ok(())
    } else {
        Err("working tree is dirty; pass --use-current-candidate to evaluate it".to_owned())
    }
}

pub fn current_head(workspace: &Path) -> Result<String, String> {
    Ok(git_checked(workspace, &["rev-parse", "HEAD"], 60)?
        .stdout
        .trim()
        .to_owned())
}

pub fn capture_patch(
    workspace: &Path,
    paths: &HistoryPaths,
    run_id: &str,
    base_ref: &str,
) -> Result<PatchSnapshot, String> {
    paths.ensure()?;
    let untracked = git_checked(
        workspace,
        &["ls-files", "--others", "--exclude-standard"],
        60,
    )?
    .stdout
    .lines()
    .filter(|line| !line.trim().is_empty())
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if !untracked.is_empty() {
        let mut args = vec!["add".to_owned(), "-N".to_owned(), "--".to_owned()];
        args.extend(untracked);
        git_dynamic(workspace, &args, 60, false)?;
    }
    let diff = git_checked(workspace, &["diff", "--binary", base_ref], 120)?.stdout;
    let _ = git_checked(workspace, &["reset", "--mixed", "HEAD"], 120)?;
    let patch_path = paths.patches.join(format!("{run_id}.patch"));
    std::fs::write(&patch_path, &diff)
        .map_err(|error| format!("failed to write {}: {error}", patch_path.display()))?;
    let sha256 = format!("{:x}", Sha256::digest(diff.as_bytes()));
    Ok(PatchSnapshot {
        path: patch_path,
        diff,
        sha256,
        base_ref: base_ref.to_owned(),
    })
}

pub fn changed_paths_from_diff(diff: &str) -> Vec<String> {
    diff.lines()
        .filter_map(|line| line.strip_prefix("diff --git a/"))
        .filter_map(|rest| rest.split(" b/").next())
        .map(ToOwned::to_owned)
        .collect()
}

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

fn git_dynamic(
    workspace: &Path,
    args: &[String],
    timeout_seconds: u64,
    check: bool,
) -> Result<CommandResult, String> {
    let mut command = vec!["git".to_owned()];
    command.extend(args.iter().cloned());
    let result = run_command(&CommandSpec::new(
        "git",
        command,
        workspace,
        None,
        timeout_seconds,
    ));
    if check && !result.passed() {
        Err(result.gate_message())
    } else {
        Ok(result)
    }
}

pub fn sleep_seconds(seconds: u64) {
    std::thread::sleep(Duration::from_secs(seconds));
}
