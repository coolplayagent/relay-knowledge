pub(super) fn worktree_overlay_base_commit(active_commit: &str) -> Option<&str> {
    active_commit
        .strip_prefix("worktree:")
        .and_then(|rest| rest.split_once(':'))
        .map(|(base_commit, _)| base_commit)
}

pub(super) fn pending_worktree_overlay_base_commit(pending_commit: &str) -> Option<&str> {
    pending_commit.strip_prefix("worktree:pending:")
}
