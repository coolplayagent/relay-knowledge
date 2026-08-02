//! Bounded and capability-safe file-content extraction.

mod budget;
mod extract;
mod read;

pub(super) use budget::reserve_content_read_with_budget;
pub(super) use extract::{FileContentEntryResult, file_content_entry, text_content_extension};
pub(super) use read::MAX_CONTENT_INDEX_BYTES;
