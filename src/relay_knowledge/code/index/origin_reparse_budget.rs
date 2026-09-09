//! Aggregate limits for origin-triggered snapshots before and during parsing.
use crate::{code::CodeIndexError, domain::CodeIndexResourceBudget};

#[derive(Default)]
pub(super) struct OriginReparseBudget {
    files: usize,
    bytes: usize,
}
impl OriginReparseBudget {
    pub(super) fn charge(&mut self, bytes: usize) -> Result<(), CodeIndexError> {
        let files = self
            .files
            .checked_add(1)
            .filter(|n| *n <= CodeIndexResourceBudget::DEFAULT_MAX_FILES_PER_BATCH);
        let total = self
            .bytes
            .checked_add(bytes)
            .filter(|n| *n <= CodeIndexResourceBudget::DEFAULT_MAX_BYTES_PER_BATCH);
        let (Some(files), Some(total)) = (files, total) else {
            return Err(CodeIndexError::InvalidInput("Python import-origin refresh exceeds the bounded file/byte budget; run a full code index so work is checkpointed and batched".into()));
        };
        self.files = files;
        self.bytes = total;
        Ok(())
    }
}
#[cfg(test)]
#[path = "origin_reparse_budget_tests.rs"]
mod tests;
