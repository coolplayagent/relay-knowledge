//! Shared per-callable and per-file work admission for structural facts.
pub(super) struct Budget<'a> {
    pub(super) remaining: usize,
    pub(super) file: &'a mut usize,
}

impl Budget<'_> {
    pub(super) fn spend(&mut self) -> Option<()> {
        self.remaining = self.remaining.checked_sub(1)?;
        *self.file = self.file.checked_sub(1)?;
        Some(())
    }
}
