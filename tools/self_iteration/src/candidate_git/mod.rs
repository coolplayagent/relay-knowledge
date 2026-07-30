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

mod command;
mod dynamic_command;
mod lifecycle;
mod patch;
mod worktree;

pub use lifecycle::{commit_candidate, reject_candidate};
pub use patch::{capture_patch, changed_paths_from_diff};
pub use worktree::{current_head, ensure_clean_worktree};

#[cfg(test)]
#[path = "git_repository_fixture.rs"]
mod git_repository_fixture;

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
