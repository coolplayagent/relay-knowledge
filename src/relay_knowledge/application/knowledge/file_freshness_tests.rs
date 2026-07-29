use super::*;

#[test]
fn path_freshness_ignores_content_only_overflow() {
    let configured_roots = vec![root()];
    let mut status = root_status(false, true);
    status.last_error = Some("file content scan byte budget exceeded".to_owned());
    let diagnostics = diagnostics_with_status(status);

    let freshness = file_freshness_diagnostics(FileFreshnessContext {
        file_index_enabled: true,
        configured_roots: &configured_roots,
        diagnostics: &diagnostics,
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        source_scope: None,
        root_id: None,
        graph_version: 0,
        query_degraded_reason: None,
        returned_paths: &[],
        content_required: false,
    });

    assert_eq!(freshness.state, FileIndexFreshnessState::Fresh);
    assert_eq!(freshness.index_lag.overflow_root_count, 0);
    assert_eq!(freshness.degraded_reason, None);
    assert!(!freshness.cursors[0].overflow);
    assert_eq!(freshness.cursors[0].last_error, None);
}

#[test]
fn content_freshness_reports_content_only_overflow() {
    let configured_roots = vec![root()];
    let mut status = root_status(false, true);
    status.last_error = Some("file content scan byte budget exceeded".to_owned());
    let diagnostics = diagnostics_with_status(status);

    let freshness = file_freshness_diagnostics(FileFreshnessContext {
        file_index_enabled: true,
        configured_roots: &configured_roots,
        diagnostics: &diagnostics,
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        source_scope: None,
        root_id: None,
        graph_version: 0,
        query_degraded_reason: None,
        returned_paths: &[],
        content_required: true,
    });

    assert_eq!(freshness.state, FileIndexFreshnessState::Overflow);
    assert_eq!(freshness.index_lag.overflow_root_count, 1);
    assert!(freshness.cursors[0].overflow);
    assert_eq!(
        freshness.degraded_reason.as_deref(),
        Some("file content scan byte budget exceeded")
    );
    assert_eq!(
        freshness.cursors[0].last_error.as_deref(),
        Some("file content scan byte budget exceeded")
    );
}

#[test]
fn content_freshness_reports_content_read_failure_as_degraded() {
    let configured_roots = vec![root()];
    let mut status = root_status(false, false);
    status.content_read_error_count = 1;
    status.last_error = Some("file content read failed".to_owned());
    let diagnostics = diagnostics_with_status(status);

    let freshness = file_freshness_diagnostics(FileFreshnessContext {
        file_index_enabled: true,
        configured_roots: &configured_roots,
        diagnostics: &diagnostics,
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        source_scope: None,
        root_id: None,
        graph_version: 0,
        query_degraded_reason: None,
        returned_paths: &[],
        content_required: true,
    });

    assert_eq!(freshness.state, FileIndexFreshnessState::Degraded);
    assert_eq!(freshness.index_lag.overflow_root_count, 0);
    assert!(!freshness.cursors[0].overflow);
    assert_eq!(
        freshness.degraded_reason.as_deref(),
        Some("file content read failed")
    );
    assert_eq!(
        freshness.cursors[0].last_error.as_deref(),
        Some("file content read failed")
    );
}

#[test]
fn content_freshness_reports_stale_read_model_cursors() {
    let configured_roots = vec![root()];
    let mut status = root_status(false, false);
    status.stale_content_cursor_count = 3;
    let diagnostics = diagnostics_with_status(status);

    let freshness = file_freshness_diagnostics(FileFreshnessContext {
        file_index_enabled: true,
        configured_roots: &configured_roots,
        diagnostics: &diagnostics,
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        source_scope: None,
        root_id: None,
        graph_version: 0,
        query_degraded_reason: None,
        returned_paths: &[],
        content_required: true,
    });

    assert_eq!(freshness.state, FileIndexFreshnessState::Stale);
    assert_eq!(
        freshness.stale_reason.as_deref(),
        Some("3 file-content read-model cursor(s) are stale")
    );
}

#[test]
fn path_freshness_ignores_stale_content_read_model_cursors() {
    let configured_roots = vec![root()];
    let mut status = root_status(false, false);
    status.stale_content_cursor_count = 3;
    let diagnostics = diagnostics_with_status(status);

    let freshness = file_freshness_diagnostics(FileFreshnessContext {
        file_index_enabled: true,
        configured_roots: &configured_roots,
        diagnostics: &diagnostics,
        freshness_policy: FreshnessPolicy::WaitUntilFresh,
        source_scope: None,
        root_id: None,
        graph_version: 0,
        query_degraded_reason: None,
        returned_paths: &[],
        content_required: false,
    });

    assert_eq!(freshness.state, FileIndexFreshnessState::Fresh);
}

fn diagnostics_with_status(status: FileIndexRootStatus) -> FileIndexDiagnostics {
    FileIndexDiagnostics {
        root_count: 1,
        indexed_file_count: status.indexed_file_count,
        missing_file_count: status.missing_file_count,
        indexed_content_count: status.indexed_content_count,
        skipped_content_count: status.skipped_content_count,
        unchanged_content_count: status.unchanged_content_count,
        stale_content_cursor_count: status.stale_content_cursor_count,
        scan_error_count: status.scan_error_count,
        content_read_error_count: status.content_read_error_count,
        truncated_root_count: usize::from(status.truncated),
        roots: vec![status],
        content_cursors: Vec::new(),
    }
}

fn root() -> FileIndexRoot {
    FileIndexRoot {
        scope_id: "local-files".to_owned(),
        root_id: "root-a".to_owned(),
        root_path: "/workspace".to_owned(),
    }
}

fn root_status(truncated: bool, content_truncated: bool) -> FileIndexRootStatus {
    FileIndexRootStatus {
        scope_id: "local-files".to_owned(),
        root_id: "root-a".to_owned(),
        root_path: "/workspace".to_owned(),
        indexed_file_count: 1,
        missing_file_count: 0,
        scan_error_count: 0,
        truncated,
        content_truncated,
        content_read_error_count: 0,
        indexed_content_count: 1,
        skipped_content_count: 0,
        unchanged_content_count: 0,
        stale_content_cursor_count: 0,
        last_indexed_at_ms: Some(10),
        last_error: None,
    }
}
