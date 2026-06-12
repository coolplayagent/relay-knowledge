use crate::domain::{CodeIndexMode, CodeIndexRequest, CodeRepositorySelector};

pub(in crate::interfaces) const WORKTREE_REF_SELECTOR: &str = "worktree";
pub(in crate::interfaces) const WORKTREE_BASE_REF_SELECTOR: &str = "HEAD";

pub(in crate::interfaces) fn mode_for_index_ref(ref_selector: &str) -> CodeIndexMode {
    if ref_selector == WORKTREE_REF_SELECTOR {
        CodeIndexMode::WorktreeOverlay
    } else {
        CodeIndexMode::Full
    }
}

pub(in crate::interfaces) fn selector_for_index_request(
    selector: CodeRepositorySelector,
    mode: &CodeIndexMode,
) -> CodeRepositorySelector {
    if *mode != CodeIndexMode::WorktreeOverlay {
        return selector;
    }

    CodeRepositorySelector {
        ref_selector: WORKTREE_BASE_REF_SELECTOR.to_owned(),
        ..selector
    }
}

pub(in crate::interfaces) fn normalize_index_request(
    mut request: CodeIndexRequest,
) -> CodeIndexRequest {
    if request.mode == CodeIndexMode::WorktreeOverlay {
        request.repository = selector_for_index_request(request.repository, &request.mode);
    }
    request
}
