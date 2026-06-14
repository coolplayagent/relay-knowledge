use crate::domain::CodebaseViewKind;

pub(super) fn snapshot_truncated(
    view_kind: CodebaseViewKind,
    files_truncated: bool,
    import_target_files_truncated: bool,
    row_counts: &[(&str, usize)],
    row_limit: usize,
) -> bool {
    let uses_files = matches!(
        view_kind,
        CodebaseViewKind::ArchitectureLayers
            | CodebaseViewKind::BusinessDomains
            | CodebaseViewKind::DependencyTour
            | CodebaseViewKind::AffectedScope
    );
    let uses_import_targets = matches!(
        view_kind,
        CodebaseViewKind::ArchitectureLayers | CodebaseViewKind::DependencyTour
    );
    (uses_files && files_truncated)
        || (uses_import_targets && import_target_files_truncated)
        || row_counts
            .iter()
            .any(|(rowset, count)| *count == row_limit && rowset_used_by_view(view_kind, rowset))
}

fn rowset_used_by_view(view_kind: CodebaseViewKind, rowset: &str) -> bool {
    matches!(
        (view_kind, rowset),
        (CodebaseViewKind::ArchitectureLayers, "imports" | "calls")
            | (
                CodebaseViewKind::BusinessDomains,
                "routes" | "feature_flags"
            )
            | (
                CodebaseViewKind::DependencyTour,
                "imports" | "calls" | "dependencies"
            )
            | (CodebaseViewKind::ProcessFlow, "routes" | "calls")
            | (CodebaseViewKind::AffectedScope, "calls")
    )
}
