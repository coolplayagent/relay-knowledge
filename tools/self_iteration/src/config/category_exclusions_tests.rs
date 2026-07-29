use super::*;

#[test]
fn exclusions_apply_to_explicit_and_default_category_sets() {
    let mut explicit = Config::parse(vec![
        "evaluate".to_owned(),
        "--categories=foundational,performance".to_owned(),
    ])
    .expect("config");
    let performance = CategorySet::single(crate::config::EvaluationCategory::Performance);

    apply_category_exclusions(&mut explicit, Some(performance.clone())).expect("exclude");

    assert_eq!(
        explicit.categories.as_ref().expect("categories").labels(),
        vec!["foundational"]
    );

    let mut defaults = Config::parse(vec!["evaluate".to_owned()]).expect("config");
    apply_category_exclusions(&mut defaults, Some(performance)).expect("exclude defaults");
    assert!(
        !defaults
            .categories
            .as_ref()
            .expect("categories")
            .contains(crate::config::EvaluationCategory::Performance)
    );
}
