use super::*;

#[test]
fn empty_request_refreshes_all_index_families() {
    let plan = IndexRefreshPlan::from_requested(Vec::new());

    assert_eq!(plan.into_kinds(), IndexKind::ALL);
}

#[test]
fn duplicate_index_kinds_are_removed_in_order() {
    let plan = IndexRefreshPlan::from_requested(vec![
        IndexKind::Vector,
        IndexKind::Vector,
        IndexKind::Bm25,
    ]);

    assert_eq!(plan.into_kinds(), [IndexKind::Vector, IndexKind::Bm25]);
}
