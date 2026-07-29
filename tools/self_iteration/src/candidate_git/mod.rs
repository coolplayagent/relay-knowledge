use std::path::Path;

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

include!("command.rs");
include!("dynamic_command.rs");
include!("worktree.rs");
include!("patch.rs");
include!("lifecycle.rs");
