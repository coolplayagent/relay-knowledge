use super::add_code_repository_totals;
use crate::domain::{CodeParseStatusCounts, CodeRepositoryTotals};

#[test]
fn repository_totals_saturate_and_accumulate_parse_statuses() {
    let mut left = CodeRepositoryTotals {
        repository_count: usize::MAX,
        indexed_file_count: 4,
        parse_status_counts: CodeParseStatusCounts {
            parsed: 2,
            partial: 1,
            text_only: 0,
            failed: usize::MAX,
        },
        ..CodeRepositoryTotals::default()
    };
    let right = CodeRepositoryTotals {
        repository_count: 1,
        indexed_file_count: 3,
        parse_status_counts: CodeParseStatusCounts {
            parsed: 5,
            partial: 2,
            text_only: 1,
            failed: 1,
        },
        ..CodeRepositoryTotals::default()
    };

    add_code_repository_totals(&mut left, right);

    assert_eq!(left.repository_count, usize::MAX);
    assert_eq!(left.indexed_file_count, 7);
    assert_eq!(left.parse_status_counts.parsed, 7);
    assert_eq!(left.parse_status_counts.partial, 3);
    assert_eq!(left.parse_status_counts.text_only, 1);
    assert_eq!(left.parse_status_counts.failed, usize::MAX);
}
