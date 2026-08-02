//! Regression tests for bounded query-result line context.

use super::{SYMBOL_CONTEXT_PREAMBLE_MAX_LINES, optional_line_range_with_symbol_context};
use crate::domain::RepositoryCodeRange;

#[test]
fn optional_line_range_requires_both_persisted_bounds() {
    assert_eq!(
        optional_line_range_with_symbol_context(Some(20), None, Some(18)),
        None
    );
    assert_eq!(
        optional_line_range_with_symbol_context(None, Some(24), Some(18)),
        None
    );
}

#[test]
fn optional_line_range_adds_only_bounded_preceding_symbol_context() {
    assert_eq!(
        optional_line_range_with_symbol_context(Some(20), Some(24), Some(17)),
        Some(RepositoryCodeRange { start: 18, end: 24 })
    );
    assert_eq!(
        optional_line_range_with_symbol_context(
            Some(40),
            Some(44),
            Some(40 - SYMBOL_CONTEXT_PREAMBLE_MAX_LINES - 2),
        ),
        Some(RepositoryCodeRange { start: 40, end: 44 })
    );
}
