use std::collections::{BTreeMap, BTreeSet};

use super::select_repository_work_items;

#[test]
fn repository_work_plan_keeps_required_set_members_and_drops_empty_others() {
    let repository_configs = BTreeMap::from([
        ("required".to_owned(), serde_json::json!({"scope": "all"})),
        ("queried".to_owned(), serde_json::json!({"scope": "all"})),
        ("empty".to_owned(), serde_json::json!({"scope": "all"})),
    ]);
    let grouped_cases = BTreeMap::from([(
        "queried".to_owned(),
        vec![serde_json::json!({"id": "definition", "kind": "definition"})],
    )]);
    let required = BTreeSet::from(["required".to_owned()]);

    let work = select_repository_work_items(
        "full",
        None,
        &repository_configs,
        &grouped_cases,
        &BTreeMap::new(),
        &required,
    );
    let names = work
        .iter()
        .map(|(name, _, _, _)| name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["queried", "required"]);
    assert!(
        work.iter()
            .find(|(name, _, _, _)| name == "required")
            .is_some_and(|(_, _, query_cases, software_cases)| {
                query_cases.is_empty() && software_cases.is_empty()
            })
    );
}
