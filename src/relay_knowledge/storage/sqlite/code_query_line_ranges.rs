use crate::domain::{CodeQueryKind, RepositoryCodeRange};

use super::code_query_rows::{CallRow, SymbolRow};

pub(super) const SYMBOL_CONTEXT_PREAMBLE_MAX_LINES: u32 = 16;

pub(super) fn symbol_result_line_range(row: &SymbolRow) -> RepositoryCodeRange {
    if row.kind == "class" {
        line_range_with_context_start(&row.line_range, row.previous_symbol_context_start)
    } else {
        row.line_range.clone()
    }
}

pub(super) fn call_result_line_range(kind: CodeQueryKind, row: &CallRow) -> RepositoryCodeRange {
    match kind {
        CodeQueryKind::Callers | CodeQueryKind::Callees => row
            .caller_line_range
            .clone()
            .unwrap_or_else(|| row.line_range.clone()),
        _ => row.line_range.clone(),
    }
}

pub(super) fn optional_line_range_with_symbol_context(
    start: Option<u32>,
    end: Option<u32>,
    previous_symbol_line_end: Option<u32>,
) -> Option<RepositoryCodeRange> {
    match (start, end) {
        (Some(start), Some(end)) => Some(line_range_with_symbol_context(
            &RepositoryCodeRange { start, end },
            previous_symbol_line_end,
        )),
        _ => None,
    }
}

fn line_range_with_symbol_context(
    line_range: &RepositoryCodeRange,
    previous_symbol_line_end: Option<u32>,
) -> RepositoryCodeRange {
    let mut contextual = line_range.clone();
    let Some(previous_symbol_line_end) = previous_symbol_line_end else {
        return contextual;
    };
    let context_start = previous_symbol_line_end.saturating_add(1);
    if context_start < contextual.start
        && contextual.start.saturating_sub(context_start) <= SYMBOL_CONTEXT_PREAMBLE_MAX_LINES
    {
        contextual.start = context_start;
    }

    contextual
}

fn line_range_with_context_start(
    line_range: &RepositoryCodeRange,
    context_start: Option<u32>,
) -> RepositoryCodeRange {
    let mut contextual = line_range.clone();
    let Some(context_start) = context_start else {
        return contextual;
    };
    if context_start < contextual.start
        && contextual.start.saturating_sub(context_start) <= SYMBOL_CONTEXT_PREAMBLE_MAX_LINES
    {
        contextual.start = context_start;
    }

    contextual
}
