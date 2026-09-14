//! Parses active and pending worktree-overlay reference identities.

pub(super) fn worktree_overlay_base_commit(active_commit: &str) -> Option<&str> {
    let (base_commit, overlay_hash) = active_commit.strip_prefix("worktree:")?.split_once(':')?;
    (!base_commit.is_empty() && base_commit != "pending" && !overlay_hash.is_empty())
        .then_some(base_commit)
}

pub(super) fn pending_worktree_overlay_base_commit(pending_commit: &str) -> Option<&str> {
    pending_commit.strip_prefix("worktree:pending:")
}

#[cfg(test)]
mod tests {
    use super::{pending_worktree_overlay_base_commit, worktree_overlay_base_commit};

    #[test]
    fn resolved_and_pending_worktree_identities_are_disjoint() {
        assert_eq!(
            worktree_overlay_base_commit("worktree:base:0123456789abcdef"),
            Some("base")
        );
        assert_eq!(
            pending_worktree_overlay_base_commit("worktree:pending:base"),
            Some("base")
        );
        assert_eq!(worktree_overlay_base_commit("worktree:pending:base"), None);
        assert_eq!(
            pending_worktree_overlay_base_commit("worktree:base:hash"),
            None
        );
    }
}
