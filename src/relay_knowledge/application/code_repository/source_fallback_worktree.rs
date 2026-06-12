use crate::{code::clean_worktree_overlay_hash, domain::CodeRepositoryStatus};

use super::worktree_ref::worktree_overlay_base_commit;

pub(super) struct SourceFallbackCommit {
    pub(super) commit: String,
    pub(super) read_worktree_overlay: bool,
}

pub(super) fn source_fallback_commit(
    status: &CodeRepositoryStatus,
) -> Option<SourceFallbackCommit> {
    let commit = status.last_indexed_commit.as_deref()?;
    let Some(base_commit) = worktree_overlay_base_commit(commit) else {
        return Some(SourceFallbackCommit {
            commit: commit.to_owned(),
            read_worktree_overlay: false,
        });
    };
    if worktree_overlay_is_clean(status, base_commit) {
        return Some(SourceFallbackCommit {
            commit: base_commit.to_owned(),
            read_worktree_overlay: false,
        });
    }

    Some(SourceFallbackCommit {
        commit: commit.to_owned(),
        read_worktree_overlay: true,
    })
}

fn worktree_overlay_is_clean(status: &CodeRepositoryStatus, base_commit: &str) -> bool {
    let clean_tree_hash = format!("worktree:{}", clean_worktree_overlay_hash(base_commit));
    status.tree_hash.as_deref() == Some(clean_tree_hash.as_str())
}
