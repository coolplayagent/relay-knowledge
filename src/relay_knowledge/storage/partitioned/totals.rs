use crate::domain::{CodeParseStatusCounts, CodeRepositoryTotals};

pub(super) fn add_code_repository_totals(
    left: &mut CodeRepositoryTotals,
    right: CodeRepositoryTotals,
) {
    left.repository_count = left.repository_count.saturating_add(right.repository_count);
    left.indexed_file_count = left
        .indexed_file_count
        .saturating_add(right.indexed_file_count);
    left.symbol_count = left.symbol_count.saturating_add(right.symbol_count);
    left.reference_count = left.reference_count.saturating_add(right.reference_count);
    left.chunk_count = left.chunk_count.saturating_add(right.chunk_count);
    left.degraded_file_count = left
        .degraded_file_count
        .saturating_add(right.degraded_file_count);
    left.parse_status_counts =
        add_parse_status_counts(left.parse_status_counts, right.parse_status_counts);
}

fn add_parse_status_counts(
    left: CodeParseStatusCounts,
    right: CodeParseStatusCounts,
) -> CodeParseStatusCounts {
    CodeParseStatusCounts {
        parsed: left.parsed.saturating_add(right.parsed),
        partial: left.partial.saturating_add(right.partial),
        text_only: left.text_only.saturating_add(right.text_only),
        failed: left.failed.saturating_add(right.failed),
    }
}
