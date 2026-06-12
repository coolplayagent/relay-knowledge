use super::super::{full_snapshot::clean_worktree_overlay_hash, ids::stable_hash64};

pub(super) struct WorktreeOverlayPlan {
    pub(super) commit: String,
    pub(super) changed_path_count: usize,
    pub(super) path_filters: Vec<String>,
    pub(super) overlay_hash_input: Vec<u8>,
    pub(super) deleted_paths: Vec<String>,
    pub(super) files_to_parse: Vec<(String, Vec<u8>)>,
    pub(super) skipped_unchanged_count: usize,
}

impl WorktreeOverlayPlan {
    pub(super) fn identity(&self) -> (String, String) {
        let overlay_hash = if self.overlay_hash_input.is_empty() {
            clean_worktree_overlay_hash(&self.commit)
        } else {
            format!("{:016x}", stable_hash64(&self.overlay_hash_input))
        };

        (
            format!("worktree:{}:{overlay_hash}", self.commit),
            format!("worktree:{overlay_hash}"),
        )
    }
}
