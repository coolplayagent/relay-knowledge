//! Partitioned checkpoint, file-index, indexing-lifecycle, and retention workflows.

pub(super) mod checkpoint;
pub(super) mod file_index;
pub(super) mod lifecycle;
pub(super) mod retention;

#[cfg(test)]
mod test_support;
