use std::collections::{BTreeMap, BTreeSet};

use crate::config::CategorySet;

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

#[test]
fn repository_work_plan_keeps_required_set_members_under_nonperformance_focus() {
    let repository_configs = BTreeMap::from([
        ("required".to_owned(), serde_json::json!({"scope": "all"})),
        ("empty".to_owned(), serde_json::json!({"scope": "all"})),
    ]);
    let foundational = CategorySet::parse("foundational").expect("foundational category");
    let required = BTreeSet::from(["required".to_owned()]);

    let work = select_repository_work_items(
        "full",
        Some(&foundational),
        &repository_configs,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &required,
    );
    let names = work
        .iter()
        .map(|(name, _, _, _)| name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["required"]);
}

#[test]
fn repository_work_plan_selects_declared_index_only_performance_targets() {
    let repository_configs = BTreeMap::from([
        (
            "index_only".to_owned(),
            serde_json::json!({
                "profile": "exhaustive",
                "scope": "all",
                "index_only_performance_target": true
            }),
        ),
        (
            "unmarked_empty".to_owned(),
            serde_json::json!({"profile": "exhaustive", "scope": "all"}),
        ),
    ]);
    let performance = CategorySet::parse("performance").expect("performance category");
    let all = CategorySet::parse("all").expect("all categories");
    let foundational = CategorySet::parse("foundational").expect("foundational category");

    for categories in [None, Some(&performance), Some(&all)] {
        let work = select_repository_work_items(
            "exhaustive",
            categories,
            &repository_configs,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        let names = work
            .iter()
            .map(|(name, _, query_cases, software_cases)| {
                assert!(query_cases.is_empty());
                assert!(software_cases.is_empty());
                name.as_str()
            })
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["index_only"]);
    }

    let foundational_work = select_repository_work_items(
        "exhaustive",
        Some(&foundational),
        &repository_configs,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    );
    assert!(foundational_work.is_empty());

    let wrong_profile_work = select_repository_work_items(
        "full",
        Some(&performance),
        &repository_configs,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    );
    assert!(wrong_profile_work.is_empty());
}
