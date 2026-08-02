//! Normalizes caller-provided graph validity ranges at commit time.

use crate::domain::{GraphVersion, GraphVersionRange};

pub(super) fn storage_version_range(
    range: GraphVersionRange,
    commit_version: GraphVersion,
) -> GraphVersionRange {
    if range.valid_from == GraphVersion::ZERO && range.valid_until.is_none() {
        GraphVersionRange::open_from(commit_version)
    } else {
        range
    }
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
