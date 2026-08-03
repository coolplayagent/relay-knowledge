use std::collections::BTreeSet;

use super::{ExportIndex, ExportTarget};

#[test]
fn intersection_merges_sorted_postings_and_excludes_import_scope() {
    let mut index = ExportIndex {
        targets: Vec::new(),
        by_key: Default::default(),
    };
    index.insert(target("scope-import", "local"), keys(["service", "serve"]));
    index.insert(
        target("scope-target", "resolved"),
        keys(["service", "serve"]),
    );
    index.insert(target("scope-noise", "parent-only"), keys(["service"]));
    index.insert(target("scope-noise", "name-only"), keys(["serve"]));

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
    index.insert(target("scope-exact", "exact"), keys(["service.serve"]));
    index.insert(
        target("scope-intersection", "intersection"),
        keys(["service", "serve"]),
    );

    let matches = index.matching_targets("scope-import", "service.serve");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].record_id, "exact");
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
