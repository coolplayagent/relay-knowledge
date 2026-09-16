//! Separates confirmed deletions from paths excluded by local source diagnostics.
use super::CodeIndexSnapshot;
impl CodeIndexSnapshot {
    /// Internal exclusion paths revoke old facts; only confirmed absence counts as deletion.
    pub fn confirmed_deleted_paths(&self) -> impl Iterator<Item = &str> {
        self.deleted_paths
            .iter()
            .filter(|path| !self.diagnostics.iter().any(|d| d.skips_path(path)))
            .map(String::as_str)
    }
}
