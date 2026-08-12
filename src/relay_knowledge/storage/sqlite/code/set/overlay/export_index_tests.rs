use std::collections::BTreeSet;

use crate::storage::StorageError;

use super::super::super::capacity::{
    MAX_MATCHING_EXPORTS_PER_IMPORT, MAX_REPOSITORY_SET_OVERLAY_EXPORTS,
};
use super::{ExportIndex, ExportTarget};

#[test]
fn intersection_merges_sorted_postings_and_excludes_import_scope() {
    let mut index = ExportIndex {
        targets: Vec::new(),
        by_key: Default::default(),
    };
    index
        .insert_bounded(target("scope-import", "local"), keys(["service", "serve"]))
        .expect("target should insert");
    index
        .insert_bounded(
            target("scope-target", "resolved"),
            keys(["service", "serve"]),
        )
        .expect("target should insert");
    index
        .insert_bounded(target("scope-noise", "parent-only"), keys(["service"]))
        .expect("target should insert");
    index
        .insert_bounded(target("scope-noise", "name-only"), keys(["serve"]))
        .expect("target should insert");

    let matches = index.matching_targets("scope-import", "service.serve");

    assert_eq!(
        matches
            .iter()
            .map(|target| target.record_id.as_str())
            .collect::<Vec<_>>(),
        ["resolved"]
    );
}

#[test]
fn exact_match_precedes_parent_name_intersection() {
    let mut index = ExportIndex {
        targets: Vec::new(),
        by_key: Default::default(),
    };
    index
        .insert_bounded(target("scope-exact", "exact"), keys(["service.serve"]))
        .expect("target should insert");
    index
        .insert_bounded(
            target("scope-intersection", "intersection"),
            keys(["service", "serve"]),
        )
        .expect("target should insert");

    let matches = index.matching_targets("scope-import", "service.serve");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].record_id, "exact");
}

#[test]
fn export_target_capacity_accepts_the_boundary_and_rejects_cap_plus_one() {
    let mut index = ExportIndex {
        targets: Vec::new(),
        by_key: Default::default(),
    };
    for target_index in 0..MAX_REPOSITORY_SET_OVERLAY_EXPORTS {
        index
            .insert_bounded(
                target("scope-target", &format!("target-{target_index}")),
                std::iter::empty(),
            )
            .expect("the bounded export target should insert");
    }

    let error = index
        .insert_bounded(
            target("scope-target", "target-over-capacity"),
            std::iter::empty(),
        )
        .expect_err("export target cap plus one should reject");

    assert!(matches!(error, StorageError::CapacityExceeded(_)));
}

#[test]
fn matching_export_window_keeps_only_the_bounded_evidence_candidates() {
    let mut index = ExportIndex {
        targets: Vec::new(),
        by_key: Default::default(),
    };
    for target_index in 0..MAX_MATCHING_EXPORTS_PER_IMPORT + 3 {
        index
            .insert_bounded(
                target("scope-target", &format!("match-{target_index:02}")),
                keys(["shared.module"]),
            )
            .expect("matching target should insert");
    }

    let matches = index.matching_targets("scope-import", "shared.module");

    assert_eq!(matches.len(), MAX_MATCHING_EXPORTS_PER_IMPORT);
    assert_eq!(
        matches.first().map(|target| target.record_id.as_str()),
        Some("match-00")
    );
    assert_eq!(
        matches.last().map(|target| target.record_id.as_str()),
        Some("match-10")
    );
}

fn target(source_scope: &str, record_id: &str) -> ExportTarget {
    ExportTarget {
        repository_id: format!("repo-{record_id}"),
        source_scope: source_scope.to_owned(),
        record_kind: "code_symbol_snapshot".to_owned(),
        record_id: record_id.to_owned(),
    }
}

fn keys<const N: usize>(values: [&str; N]) -> BTreeSet<String> {
    values.into_iter().map(str::to_owned).collect()
}
