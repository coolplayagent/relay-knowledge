use std::collections::BTreeSet;

use crate::{
    api::{
        FileIndexFreshnessCursor, FileIndexFreshnessDiagnostics, FileIndexFreshnessState,
        FileIndexLag,
    },
    domain::FreshnessPolicy,
    storage::{FileIndexDiagnostics, FileIndexRoot, FileIndexRootStatus, FileSearchHit},
};

pub(super) struct FileFreshnessContext<'a> {
    pub(super) file_index_enabled: bool,
    pub(super) configured_roots: &'a [FileIndexRoot],
    pub(super) diagnostics: &'a FileIndexDiagnostics,
    pub(super) freshness_policy: FreshnessPolicy,
    pub(super) source_scope: Option<String>,
    pub(super) root_id: Option<String>,
    pub(super) graph_version: u64,
    pub(super) query_degraded_reason: Option<String>,
    pub(super) returned_hits: &'a [FileSearchHit],
}

pub(super) fn file_freshness_diagnostics(
    context: FileFreshnessContext<'_>,
) -> FileIndexFreshnessDiagnostics {
    let selected = selected_root_statuses(
        context.configured_roots,
        context.diagnostics,
        context.source_scope.as_deref(),
        context.root_id.as_deref(),
    );
    let pending_roots = pending_configured_roots(
        context.configured_roots,
        context.diagnostics,
        context.source_scope.as_deref(),
        context.root_id.as_deref(),
    );
    let cursors = selected
        .iter()
        .map(|status| FileIndexFreshnessCursor {
            source_scope: status.scope_id.clone(),
            root_id: status.root_id.clone(),
            root_path: status.root_path.clone(),
            backend: "bounded_scan".to_owned(),
            scan_watermark_ms: status.last_indexed_at_ms,
            indexed_file_count: status.indexed_file_count,
            missing_file_count: status.missing_file_count,
            scan_error_count: status.scan_error_count,
            overflow: status.truncated,
            last_error: status.last_error.clone(),
        })
        .collect::<Vec<_>>();
    let stale_root_count = selected
        .iter()
        .filter(|status| status.last_indexed_at_ms.is_none())
        .count()
        .saturating_add(pending_roots.len());
    let overflow_root_count = selected.iter().filter(|status| status.truncated).count();
    let scan_error_count = selected
        .iter()
        .map(|status| status.scan_error_count)
        .sum::<usize>();
    let missing_file_count = selected
        .iter()
        .map(|status| status.missing_file_count)
        .sum::<usize>();
    let indexed_root_count = selected
        .iter()
        .filter(|status| status.last_indexed_at_ms.is_some())
        .count();
    let configured_root_count = selected.len().saturating_add(pending_roots.len());
    let degraded_reason = context
        .query_degraded_reason
        .or_else(|| selected.iter().find_map(|status| status.last_error.clone()));
    let state = file_freshness_state(
        context.file_index_enabled,
        configured_root_count,
        stale_root_count,
        overflow_root_count,
        scan_error_count,
        degraded_reason.as_ref(),
    );
    let stale_reason = stale_reason_for_state(state, stale_root_count, overflow_root_count);
    let direct_source_read_required = !matches!(
        state,
        FileIndexFreshnessState::Fresh | FileIndexFreshnessState::Paused
    );
    let bounded_rescan_required = matches!(
        state,
        FileIndexFreshnessState::Pending
            | FileIndexFreshnessState::Stale
            | FileIndexFreshnessState::Degraded
            | FileIndexFreshnessState::Overflow
    );
    let direct_source_read_paths = returned_paths(context.returned_hits);

    FileIndexFreshnessDiagnostics {
        state,
        freshness_policy: context.freshness_policy,
        graph_version: context.graph_version,
        source_scope: context.source_scope,
        root_id: context.root_id,
        stale_reason,
        degraded_reason,
        index_lag: FileIndexLag {
            configured_root_count,
            indexed_root_count,
            pending_root_count: pending_roots.len(),
            stale_root_count,
            overflow_root_count,
            missing_file_count,
            pending_task_count: 0,
        },
        cursors,
        direct_source_read_required,
        bounded_rescan_required,
        direct_source_read_paths,
        agent_instructions: agent_instructions(
            state,
            bounded_rescan_required,
            direct_source_read_required,
        ),
    }
}

fn selected_root_statuses(
    configured_roots: &[FileIndexRoot],
    diagnostics: &FileIndexDiagnostics,
    source_scope: Option<&str>,
    root_id: Option<&str>,
) -> Vec<FileIndexRootStatus> {
    let configured = configured_roots
        .iter()
        .map(|root| (root.scope_id.clone(), root.root_id.clone()))
        .collect::<BTreeSet<_>>();

    diagnostics
        .roots
        .iter()
        .filter(|status| {
            configured.contains(&(status.scope_id.clone(), status.root_id.clone()))
                && source_scope.is_none_or(|scope| status.scope_id == scope)
                && root_id.is_none_or(|root| status.root_id == root)
        })
        .cloned()
        .collect()
}

fn pending_configured_roots(
    configured_roots: &[FileIndexRoot],
    diagnostics: &FileIndexDiagnostics,
    source_scope: Option<&str>,
    root_id: Option<&str>,
) -> Vec<FileIndexRoot> {
    let known = diagnostics
        .roots
        .iter()
        .map(|status| (status.scope_id.clone(), status.root_id.clone()))
        .collect::<BTreeSet<_>>();

    configured_roots
        .iter()
        .filter(|root| {
            source_scope.is_none_or(|scope| root.scope_id == scope)
                && root_id.is_none_or(|filter| root.root_id == filter)
                && !known.contains(&(root.scope_id.clone(), root.root_id.clone()))
        })
        .cloned()
        .collect()
}

fn file_freshness_state(
    enabled: bool,
    configured_root_count: usize,
    stale_root_count: usize,
    overflow_root_count: usize,
    scan_error_count: usize,
    degraded_reason: Option<&String>,
) -> FileIndexFreshnessState {
    if !enabled && configured_root_count == 0 {
        FileIndexFreshnessState::Paused
    } else if overflow_root_count > 0 {
        FileIndexFreshnessState::Overflow
    } else if scan_error_count > 0 || degraded_reason.is_some() {
        FileIndexFreshnessState::Degraded
    } else if stale_root_count > 0 {
        FileIndexFreshnessState::Pending
    } else if configured_root_count == 0 {
        FileIndexFreshnessState::Stale
    } else {
        FileIndexFreshnessState::Fresh
    }
}

fn stale_reason_for_state(
    state: FileIndexFreshnessState,
    stale_root_count: usize,
    overflow_root_count: usize,
) -> Option<String> {
    match state {
        FileIndexFreshnessState::Pending => Some(format!(
            "{stale_root_count} configured file-index root(s) have not completed a scan"
        )),
        FileIndexFreshnessState::Stale => Some("no matching file-index root is fresh".to_owned()),
        FileIndexFreshnessState::Overflow => Some(format!(
            "{overflow_root_count} file-index root scan(s) overflowed the bounded scan budget"
        )),
        FileIndexFreshnessState::Degraded => {
            Some("file-index root scan or query is degraded".to_owned())
        }
        FileIndexFreshnessState::Paused | FileIndexFreshnessState::Fresh => None,
    }
}

fn returned_paths(hits: &[FileSearchHit]) -> Vec<String> {
    hits.iter()
        .map(|hit| hit.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn agent_instructions(
    state: FileIndexFreshnessState,
    bounded_rescan_required: bool,
    direct_source_read_required: bool,
) -> Vec<String> {
    let mut instructions = Vec::new();
    if bounded_rescan_required {
        instructions.push(
            "Run a bounded file index scan before trusting local file-index freshness.".to_owned(),
        );
    }
    if direct_source_read_required {
        instructions.push(format!(
            "File-index state is {}; read direct source paths before editing or citing changed files.",
            state_label(state)
        ));
    }

    instructions
}

fn state_label(state: FileIndexFreshnessState) -> &'static str {
    match state {
        FileIndexFreshnessState::Fresh => "fresh",
        FileIndexFreshnessState::Pending => "pending",
        FileIndexFreshnessState::Paused => "paused",
        FileIndexFreshnessState::Stale => "stale",
        FileIndexFreshnessState::Degraded => "degraded",
        FileIndexFreshnessState::Overflow => "overflow",
    }
}
