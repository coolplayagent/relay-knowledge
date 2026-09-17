//! Lazy per-file line lookup, preserving CR, LF and CRLF prefix semantics.
use std::cell::OnceCell;

#[derive(Default)]
pub(crate) struct LineIndex(OnceCell<Vec<usize>>);

impl super::super::FeatureFlagFileInput<'_> {
    pub(super) fn line_number(&self, offset: usize) -> usize {
        let breaks = self.line_index.0.get_or_init(|| {
            let bytes = self.content.as_bytes();
            bytes
                .iter()
                .enumerate()
                .filter_map(|(index, byte)| {
                    (*byte == b'\r'
                        || (*byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r')))
                        .then_some(index)
                })
                .collect()
        });
        1 + breaks.partition_point(|index| *index < offset)
    }
}

#[cfg(test)]
#[path = "line_index_tests.rs"]
mod tests;
