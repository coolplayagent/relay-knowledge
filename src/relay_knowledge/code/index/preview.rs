//! Read-only parser validation of a pinned repository scope, using index batch budgets.

use std::time::{Duration, Instant};

use crate::{
    code::{CodeIndexError, source::layout::preview_repository_layout},
    domain::{
        CodeIndexResourceBudget, CodeRepositoryDegradedFile, CodeRepositoryRegistration,
        CodeRepositoryScopePreview, CodeRepositorySelector,
    },
};

use super::prepare_full_index_plan;

const PREVIEW_PARSE_TIMEOUT: Duration = Duration::from_secs(120);

/// Validates the selected snapshot with the indexing parser without persisting facts.
pub fn preview_repository_scope(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
) -> Result<CodeRepositoryScopePreview, CodeIndexError> {
    let deadline = Instant::now() + PREVIEW_PARSE_TIMEOUT;
    preview_repository_scope_cancellable(registration, selector, || Instant::now() >= deadline)
}

pub(crate) fn preview_repository_scope_cancellable(
    registration: &CodeRepositoryRegistration,
    selector: &CodeRepositorySelector,
    cancelled: impl Fn() -> bool,
) -> Result<CodeRepositoryScopePreview, CodeIndexError> {
    let check_cancelled = || {
        if cancelled() {
            Err(CodeIndexError::Io(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "repository preview cancelled or timed out; no degradation preview is available",
            )))
        } else {
            Ok(())
        }
    };
    check_cancelled()?;
    let mut preview = preview_repository_layout(registration, selector)?;
    check_cancelled()?;
    let mut pinned = selector.clone();
    pinned.ref_selector.clone_from(&preview.resolved_commit_sha);
    let mut plan = prepare_full_index_plan(
        registration.clone(),
        pinned,
        CodeIndexResourceBudget::default(),
    )?;
    let session = plan.session();
    if session.resolved_commit_sha != preview.resolved_commit_sha
        || session.tree_hash != preview.tree_hash
        || session.total_path_count != preview.selected_file_count
    {
        return Err(CodeIndexError::InvalidInput(
            "repository scope changed during preview; retry against a stable snapshot".to_owned(),
        ));
    }
    let mut parsed = 0usize;
    loop {
        check_cancelled()?;
        let (next, batch) = plan.parse_next_batch()?;
        check_cancelled()?;
        plan = next;
        if plan.needs_source_replan {
            return Err(CodeIndexError::InvalidInput(
                "repository source changed or became unreadable during preview; retry against a stable snapshot"
                    .to_owned(),
            ));
        }
        let Some(batch) = batch else { break };
        parsed += batch.files.len();
        for diagnostic in batch.diagnostics {
            if preview
                .expected_degraded_files
                .iter()
                .any(|file| file.path == diagnostic.path)
            {
                continue;
            }
            if preview.expected_degraded_files.len() == CodeRepositoryScopePreview::MAX_DETAIL_FILES
            {
                preview.expected_degraded_files_truncated = true;
                return Ok(preview);
            }
            preview
                .expected_degraded_files
                .push(CodeRepositoryDegradedFile {
                    path: diagnostic.path,
                    reason: diagnostic.message,
                });
        }
        // Drop each batch's facts immediately; preview never accumulates a repository graph.
    }
    if parsed != preview.selected_file_count {
        return Err(CodeIndexError::Invariant(
            "preview parser did not cover the selected file set".to_owned(),
        ));
    }
    Ok(preview)
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
