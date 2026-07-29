use std::path::Path;

use super::command::git_checked;

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

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod worktree_tests;
